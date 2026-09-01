//! The `migrate` subcommand.
//!
//! A service migrates its own schema when it starts, so this exists for the two
//! times that is not what you want: seeing what a deploy is about to do, and
//! undoing what one already did.

use anyhow::{Context, Result, bail};
use austeris_common::{Config, db, migrate};

use crate::service::Service;

/// Arguments to `austeris migrate`.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Which service's schema to act on. Every service when omitted.
    #[arg(long)]
    pub service: Option<Service>,

    /// Print what would be applied, and apply nothing.
    #[arg(long)]
    pub dry_run: bool,

    /// Roll back to this migration, undoing everything newer.
    ///
    /// The version named is the one to stop at, not the one to undo. `-1` undoes
    /// everything. Refused without `--service`: rolling several schemas back at
    /// once is never what someone means.
    ///
    /// `allow_hyphen_values` because the most useful value starts with one:
    /// without it `--undo-to -1` is read as an unknown flag and refused, and
    /// the operator finds that out while rolling a release back.
    #[arg(long, value_name = "VERSION", allow_hyphen_values = true)]
    pub undo_to: Option<i64>,
}

/// Runs the subcommand.
///
/// # Errors
///
/// Returns an error when a schema cannot be reached, a migration fails, or the
/// arguments contradict each other.
pub async fn run(args: &Args) -> Result<()> {
    if args.undo_to.is_some() && args.service.is_none() {
        bail!("--undo-to needs --service: rolling every schema back at once is never what someone means");
    }
    if args.undo_to.is_some() && args.dry_run {
        bail!("--undo-to and --dry-run contradict each other");
    }

    let config = Config::from_env()?;

    let services: Vec<Service> = match args.service {
        Some(service) => vec![service],
        None => Service::ALL.to_vec(),
    };

    for service in services {
        let (Some(schema), Some(migrator)) = (service.schema(), service.migrator()) else {
            // The gateway owns no schema. Saying so beats silently doing
            // nothing when someone asks for it by name.
            if args.service.is_some() {
                println!("{service} owns no schema; there is nothing to migrate");
            }
            continue;
        };

        let pool = db::connect(&config, schema).await.with_context(|| format!("connecting for {service}"))?;

        if let Some(target) = args.undo_to {
            migrate::undo(&pool, migrator, target)
                .await
                .with_context(|| format!("rolling {service} back to {target}"))?;
            println!("{service}: rolled back to {target}");
            continue;
        }

        let plan = migrate::plan(&pool, schema, migrator).await.with_context(|| format!("planning {service}"))?;

        if plan.is_up_to_date() {
            println!(
                "{service}: up to date at {}",
                plan.current.map_or_else(|| "no version".to_owned(), |v| v.to_string())
            );
            continue;
        }

        println!("{service}: {} migration(s) pending", plan.pending.len());
        for (version, description) in &plan.pending {
            println!("  {version}  {description}");
        }

        if args.dry_run {
            continue;
        }

        migrate::run(&pool, migrator).await.with_context(|| format!("migrating {service}"))?;
        println!("{service}: applied");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::Args;

    /// A parser standing in for the real CLI, so these test argument parsing
    /// rather than the whole binary.
    #[derive(Debug, Parser)]
    struct Cli {
        #[command(flatten)]
        args: Args,
    }

    #[test]
    fn a_negative_target_is_a_value_and_not_a_flag() {
        // `--undo-to -1` undoes everything, and is the form an operator reaches
        // for while rolling a release back. Without `allow_hyphen_values` clap
        // reads the `-1` as an unknown flag and refuses the command - which is
        // exactly when nobody wants to be debugging their own tooling.
        let cli = Cli::try_parse_from(["austeris", "--service", "identity", "--undo-to", "-1"]).expect("parsing");
        assert_eq!(cli.args.undo_to, Some(-1));
    }

    #[test]
    fn the_equals_form_parses_the_same_way() {
        let cli = Cli::try_parse_from(["austeris", "--undo-to=-1"]).expect("parsing");
        assert_eq!(cli.args.undo_to, Some(-1));
    }

    #[test]
    fn a_positive_target_still_parses() {
        let cli = Cli::try_parse_from(["austeris", "--undo-to", "20260901120001"]).expect("parsing");
        assert_eq!(cli.args.undo_to, Some(20_260_901_120_001));
    }

    #[test]
    fn no_arguments_means_migrate_everything_forward() {
        let cli = Cli::try_parse_from(["austeris"]).expect("parsing");
        assert!(cli.args.service.is_none() && cli.args.undo_to.is_none() && !cli.args.dry_run);
    }
}

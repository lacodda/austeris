--
-- PostgreSQL database dump
--

\restrict Z03i2jd3Zmx8dZI6brhp526tNPsah6JxlRlUHYglqDP0Qu946GIvYJZyWL544lU

-- Dumped from database version 17.5
-- Dumped by pg_dump version 18.6

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET transaction_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: assets; Type: SCHEMA; Schema: -; Owner: tracker
--

CREATE SCHEMA assets;


ALTER SCHEMA assets OWNER TO tracker;

SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: assets; Type: TABLE; Schema: assets; Owner: tracker
--

CREATE TABLE assets.assets (
    id integer NOT NULL,
    symbol character varying(10) NOT NULL,
    name character varying(50) NOT NULL,
    cmc_id integer NOT NULL
);


ALTER TABLE assets.assets OWNER TO tracker;

--
-- Name: assets_id_seq; Type: SEQUENCE; Schema: assets; Owner: tracker
--

CREATE SEQUENCE assets.assets_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE assets.assets_id_seq OWNER TO tracker;

--
-- Name: assets_id_seq; Type: SEQUENCE OWNED BY; Schema: assets; Owner: tracker
--

ALTER SEQUENCE assets.assets_id_seq OWNED BY assets.assets.id;


--
-- Name: _sqlx_migrations; Type: TABLE; Schema: public; Owner: tracker
--

CREATE TABLE public._sqlx_migrations (
    version bigint NOT NULL,
    description text NOT NULL,
    installed_on timestamp with time zone DEFAULT now() NOT NULL,
    success boolean NOT NULL,
    checksum bytea NOT NULL,
    execution_time bigint NOT NULL
);


ALTER TABLE public._sqlx_migrations OWNER TO tracker;

--
-- Name: asset_prices; Type: TABLE; Schema: public; Owner: tracker
--

CREATE TABLE public.asset_prices (
    id integer NOT NULL,
    asset_id integer,
    price_usd double precision NOT NULL,
    "timestamp" timestamp without time zone DEFAULT CURRENT_TIMESTAMP NOT NULL
);


ALTER TABLE public.asset_prices OWNER TO tracker;

--
-- Name: asset_prices_id_seq; Type: SEQUENCE; Schema: public; Owner: tracker
--

CREATE SEQUENCE public.asset_prices_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.asset_prices_id_seq OWNER TO tracker;

--
-- Name: asset_prices_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: tracker
--

ALTER SEQUENCE public.asset_prices_id_seq OWNED BY public.asset_prices.id;


--
-- Name: assets; Type: TABLE; Schema: public; Owner: tracker
--

CREATE TABLE public.assets (
    id integer NOT NULL,
    symbol character varying(10) NOT NULL,
    name character varying(50) NOT NULL,
    cmc_id integer NOT NULL,
    decimals integer,
    rank integer,
    created_at timestamp without time zone DEFAULT CURRENT_TIMESTAMP NOT NULL
);


ALTER TABLE public.assets OWNER TO tracker;

--
-- Name: assets_id_seq; Type: SEQUENCE; Schema: public; Owner: tracker
--

CREATE SEQUENCE public.assets_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.assets_id_seq OWNER TO tracker;

--
-- Name: assets_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: tracker
--

ALTER SEQUENCE public.assets_id_seq OWNED BY public.assets.id;


--
-- Name: portfolio_snapshots; Type: TABLE; Schema: public; Owner: tracker
--

CREATE TABLE public.portfolio_snapshots (
    id integer NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    assets jsonb NOT NULL
);


ALTER TABLE public.portfolio_snapshots OWNER TO tracker;

--
-- Name: portfolio_snapshots_id_seq; Type: SEQUENCE; Schema: public; Owner: tracker
--

CREATE SEQUENCE public.portfolio_snapshots_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.portfolio_snapshots_id_seq OWNER TO tracker;

--
-- Name: portfolio_snapshots_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: tracker
--

ALTER SEQUENCE public.portfolio_snapshots_id_seq OWNED BY public.portfolio_snapshots.id;


--
-- Name: transactions; Type: TABLE; Schema: public; Owner: tracker
--

CREATE TABLE public.transactions (
    id integer NOT NULL,
    asset_id integer,
    wallet_id integer,
    amount double precision NOT NULL,
    price double precision NOT NULL,
    fee double precision,
    type character varying(4) NOT NULL,
    notes text,
    created_at timestamp without time zone DEFAULT CURRENT_TIMESTAMP NOT NULL,
    CONSTRAINT transactions_type_check CHECK (((type)::text = ANY ((ARRAY['BUY'::character varying, 'SELL'::character varying])::text[])))
);


ALTER TABLE public.transactions OWNER TO tracker;

--
-- Name: transactions_id_seq; Type: SEQUENCE; Schema: public; Owner: tracker
--

CREATE SEQUENCE public.transactions_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.transactions_id_seq OWNER TO tracker;

--
-- Name: transactions_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: tracker
--

ALTER SEQUENCE public.transactions_id_seq OWNED BY public.transactions.id;


--
-- Name: wallets; Type: TABLE; Schema: public; Owner: tracker
--

CREATE TABLE public.wallets (
    id integer NOT NULL,
    name character varying(50) NOT NULL,
    type character varying(20) NOT NULL,
    address character varying(255),
    created_at timestamp without time zone DEFAULT CURRENT_TIMESTAMP NOT NULL
);


ALTER TABLE public.wallets OWNER TO tracker;

--
-- Name: wallets_id_seq; Type: SEQUENCE; Schema: public; Owner: tracker
--

CREATE SEQUENCE public.wallets_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.wallets_id_seq OWNER TO tracker;

--
-- Name: wallets_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: tracker
--

ALTER SEQUENCE public.wallets_id_seq OWNED BY public.wallets.id;


--
-- Name: assets id; Type: DEFAULT; Schema: assets; Owner: tracker
--

ALTER TABLE ONLY assets.assets ALTER COLUMN id SET DEFAULT nextval('assets.assets_id_seq'::regclass);


--
-- Name: asset_prices id; Type: DEFAULT; Schema: public; Owner: tracker
--

ALTER TABLE ONLY public.asset_prices ALTER COLUMN id SET DEFAULT nextval('public.asset_prices_id_seq'::regclass);


--
-- Name: assets id; Type: DEFAULT; Schema: public; Owner: tracker
--

ALTER TABLE ONLY public.assets ALTER COLUMN id SET DEFAULT nextval('public.assets_id_seq'::regclass);


--
-- Name: portfolio_snapshots id; Type: DEFAULT; Schema: public; Owner: tracker
--

ALTER TABLE ONLY public.portfolio_snapshots ALTER COLUMN id SET DEFAULT nextval('public.portfolio_snapshots_id_seq'::regclass);


--
-- Name: transactions id; Type: DEFAULT; Schema: public; Owner: tracker
--

ALTER TABLE ONLY public.transactions ALTER COLUMN id SET DEFAULT nextval('public.transactions_id_seq'::regclass);


--
-- Name: wallets id; Type: DEFAULT; Schema: public; Owner: tracker
--

ALTER TABLE ONLY public.wallets ALTER COLUMN id SET DEFAULT nextval('public.wallets_id_seq'::regclass);


--
-- Data for Name: assets; Type: TABLE DATA; Schema: assets; Owner: tracker
--

COPY assets.assets (id, symbol, name, cmc_id) FROM stdin;
1	BTC	Bitcoin	1
\.


--
-- Data for Name: _sqlx_migrations; Type: TABLE DATA; Schema: public; Owner: tracker
--

COPY public._sqlx_migrations (version, description, installed_on, success, checksum, execution_time) FROM stdin;
2025030100	create assets	2025-03-01 10:00:00+00	t	\\xdeadbeef	1203456
2025030800	create asset prices	2025-03-08 10:00:00+00	t	\\x0102030405	803456
\.


--
-- Data for Name: asset_prices; Type: TABLE DATA; Schema: public; Owner: tracker
--

COPY public.asset_prices (id, asset_id, price_usd, "timestamp") FROM stdin;
1	1	85000.12	2025-03-05 14:00:00
2	1	86500.5	2025-03-06 19:59:59
3	1	84000	2025-03-20 15:00:00
4	1	83000.75	2025-04-15 11:00:00
5	2	2150	2025-03-06 19:59:59
6	2	1980.25	2025-03-20 15:00:00
7	2	1590.5	2025-04-15 11:00:00
8	3	0.9998	2025-03-06 19:59:59
9	3	1.0001	2025-03-20 15:00:00
10	3	0.9999	2025-04-15 11:00:00
11	4	125.5	2025-04-15 11:00:00
12	5	0.17	2025-04-15 11:00:00
\.


--
-- Data for Name: assets; Type: TABLE DATA; Schema: public; Owner: tracker
--

COPY public.assets (id, symbol, name, cmc_id, decimals, rank, created_at) FROM stdin;
1	BTC	Bitcoin	1	8	1	2025-03-01 10:00:00
2	ETH	Ethereum	1027	18	2	2025-03-01 10:00:00
3	USDT	Tether USDt	825	6	3	2025-03-01 10:00:00
4	SOL	Solana	5426	9	5	2025-03-01 10:00:00
5	DOGE	Dogecoin	74	8	8	2025-03-01 10:00:00
\.


--
-- Data for Name: portfolio_snapshots; Type: TABLE DATA; Schema: public; Owner: tracker
--

COPY public.portfolio_snapshots (id, created_at, assets) FROM stdin;
1	2025-03-06 20:00:00	[{"amount": 0.03, "cmc_id": 1, "symbol": "BTC"}, {"amount": 298.29999999999995, "cmc_id": 825, "symbol": "USDT"}]
2	2025-03-21 01:00:00	[{"amount": 0.025, "cmc_id": 1, "symbol": "BTC"}, {"amount": 0.5, "cmc_id": 1027, "symbol": "ETH"}, {"amount": 717.8799999999999, "cmc_id": 825, "symbol": "USDT"}]
3	2025-03-21 02:30:00	[{"amount": 0.025, "cmc_id": 1, "symbol": "BTC"}, {"amount": 0.5, "cmc_id": 1027, "symbol": "ETH"}, {"amount": 717.8799999999999, "cmc_id": 825, "symbol": "USDT"}]
4	2025-04-15 12:00:00	[{"amount": 0.025, "cmc_id": 1, "symbol": "BTC"}, {"amount": 0.6000000000000001, "cmc_id": 1027, "symbol": "ETH"}, {"amount": 6, "cmc_id": 5426, "symbol": "SOL"}, {"amount": 717.8799999999999, "cmc_id": 825, "symbol": "USDT"}]
\.


--
-- Data for Name: transactions; Type: TABLE DATA; Schema: public; Owner: tracker
--

COPY public.transactions (id, asset_id, wallet_id, amount, price, fee, type, notes, created_at) FROM stdin;
1	3	1	2000	1	\N	BUY	first deposit	2025-03-03 13:00:00.512341
2	1	1	0.02	85000	1.7	BUY	\N	2025-03-05 14:10:00.123456
3	3	1	1701.7	1	\N	SELL	\N	2025-03-05 14:12:31.000001
4	2	1	0.5	2100	1.05	BUY	dip buy ☕ café\twith a tab	2025-03-10 09:00:00
5	1	1	0.005	84000	0.42	SELL	\N	2025-03-20 16:00:00
6	3	1	419.58	1	\N	BUY	\N	2025-03-20 16:01:00
7	4	1	10	0	\N	BUY	staking reward	2025-04-02 23:30:00
8	4	1	4	120	0	SELL	\N	2025-04-03 10:00:00
9	2	1	0.2	1600	0.32	SELL	\N	2025-04-10 12:00:00
10	1	2	0.01	86000	\N	BUY	cold storage\\vault	2025-03-06 08:00:00
11	2	2	0.30000000000000004	2000	\N	BUY	\N	2025-04-12 15:00:00
\.


--
-- Data for Name: wallets; Type: TABLE DATA; Schema: public; Owner: tracker
--

COPY public.wallets (id, name, type, address, created_at) FROM stdin;
1	Binance	exchange	\N	2026-10-31 22:52:05.537998
2	Ledger Nano	hardware	\N	2026-10-31 22:52:05.537998
3	Bybit	exchange	\N	2025-03-01 10:00:00
\.


--
-- Name: assets_id_seq; Type: SEQUENCE SET; Schema: assets; Owner: tracker
--

SELECT pg_catalog.setval('assets.assets_id_seq', 1, true);


--
-- Name: asset_prices_id_seq; Type: SEQUENCE SET; Schema: public; Owner: tracker
--

SELECT pg_catalog.setval('public.asset_prices_id_seq', 12, true);


--
-- Name: assets_id_seq; Type: SEQUENCE SET; Schema: public; Owner: tracker
--

SELECT pg_catalog.setval('public.assets_id_seq', 5, true);


--
-- Name: portfolio_snapshots_id_seq; Type: SEQUENCE SET; Schema: public; Owner: tracker
--

SELECT pg_catalog.setval('public.portfolio_snapshots_id_seq', 4, true);


--
-- Name: transactions_id_seq; Type: SEQUENCE SET; Schema: public; Owner: tracker
--

SELECT pg_catalog.setval('public.transactions_id_seq', 11, true);


--
-- Name: wallets_id_seq; Type: SEQUENCE SET; Schema: public; Owner: tracker
--

SELECT pg_catalog.setval('public.wallets_id_seq', 3, true);


--
-- Name: assets assets_cmc_id_key; Type: CONSTRAINT; Schema: assets; Owner: tracker
--

ALTER TABLE ONLY assets.assets
    ADD CONSTRAINT assets_cmc_id_key UNIQUE (cmc_id);


--
-- Name: assets assets_pkey; Type: CONSTRAINT; Schema: assets; Owner: tracker
--

ALTER TABLE ONLY assets.assets
    ADD CONSTRAINT assets_pkey PRIMARY KEY (id);


--
-- Name: _sqlx_migrations _sqlx_migrations_pkey; Type: CONSTRAINT; Schema: public; Owner: tracker
--

ALTER TABLE ONLY public._sqlx_migrations
    ADD CONSTRAINT _sqlx_migrations_pkey PRIMARY KEY (version);


--
-- Name: asset_prices asset_prices_pkey; Type: CONSTRAINT; Schema: public; Owner: tracker
--

ALTER TABLE ONLY public.asset_prices
    ADD CONSTRAINT asset_prices_pkey PRIMARY KEY (id);


--
-- Name: assets assets_cmc_id_key; Type: CONSTRAINT; Schema: public; Owner: tracker
--

ALTER TABLE ONLY public.assets
    ADD CONSTRAINT assets_cmc_id_key UNIQUE (cmc_id);


--
-- Name: assets assets_pkey; Type: CONSTRAINT; Schema: public; Owner: tracker
--

ALTER TABLE ONLY public.assets
    ADD CONSTRAINT assets_pkey PRIMARY KEY (id);


--
-- Name: portfolio_snapshots portfolio_snapshots_pkey; Type: CONSTRAINT; Schema: public; Owner: tracker
--

ALTER TABLE ONLY public.portfolio_snapshots
    ADD CONSTRAINT portfolio_snapshots_pkey PRIMARY KEY (id);


--
-- Name: transactions transactions_pkey; Type: CONSTRAINT; Schema: public; Owner: tracker
--

ALTER TABLE ONLY public.transactions
    ADD CONSTRAINT transactions_pkey PRIMARY KEY (id);


--
-- Name: asset_prices unique_asset_timestamp; Type: CONSTRAINT; Schema: public; Owner: tracker
--

ALTER TABLE ONLY public.asset_prices
    ADD CONSTRAINT unique_asset_timestamp UNIQUE (asset_id, "timestamp");


--
-- Name: wallets wallets_pkey; Type: CONSTRAINT; Schema: public; Owner: tracker
--

ALTER TABLE ONLY public.wallets
    ADD CONSTRAINT wallets_pkey PRIMARY KEY (id);


--
-- Name: idx_asset_prices_asset_id_timestamp; Type: INDEX; Schema: public; Owner: tracker
--

CREATE INDEX idx_asset_prices_asset_id_timestamp ON public.asset_prices USING btree (asset_id, "timestamp" DESC);


--
-- Name: idx_assets_rank; Type: INDEX; Schema: public; Owner: tracker
--

CREATE INDEX idx_assets_rank ON public.assets USING btree (rank);


--
-- Name: asset_prices asset_prices_asset_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: tracker
--

ALTER TABLE ONLY public.asset_prices
    ADD CONSTRAINT asset_prices_asset_id_fkey FOREIGN KEY (asset_id) REFERENCES public.assets(id);


--
-- Name: transactions transactions_asset_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: tracker
--

ALTER TABLE ONLY public.transactions
    ADD CONSTRAINT transactions_asset_id_fkey FOREIGN KEY (asset_id) REFERENCES public.assets(id);


--
-- Name: transactions transactions_wallet_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: tracker
--

ALTER TABLE ONLY public.transactions
    ADD CONSTRAINT transactions_wallet_id_fkey FOREIGN KEY (wallet_id) REFERENCES public.wallets(id);


--
-- PostgreSQL database dump complete
--

\unrestrict Z03i2jd3Zmx8dZI6brhp526tNPsah6JxlRlUHYglqDP0Qu946GIvYJZyWL544lU


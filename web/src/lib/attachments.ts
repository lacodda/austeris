/*
 * Files kept with an entry, as a screen shows them.
 *
 * The server reads what a file is from its bytes and says so in `media_type`;
 * nothing here second-guesses it. What is decided here is only what a browser
 * can draw: a phone's HEIC photo is kept and can be saved, but most browsers
 * cannot show one, so it is listed by name rather than as a broken picture.
 */

/** What the file chooser offers: pictures - with the camera, on a phone - PDFs
 * and XML receipts. The server refuses anything else whatever this says. */
export const ACCEPT = 'image/*,application/pdf,.pdf,application/xml,text/xml,.xml'

/** Whether a browser can draw a file of this type as a picture. */
export function isPicture(mediaType: string): boolean {
  return mediaType === 'image/jpeg' || mediaType === 'image/png' || mediaType === 'image/webp'
}

/** A file's size the way a person reads it: `340 KB`, `2.4 MB`. */
export function formatSize(bytes: number, locale: string): string {
  // Kilobytes all the way down: a receipt's XML of a few hundred bytes reads
  // better as `0.4 kB` than in a unit nobody counts files in.
  const [value, unit] = bytes >= 1024 * 1024 ? [bytes / (1024 * 1024), 'megabyte'] : [bytes / 1024, 'kilobyte']
  return new Intl.NumberFormat(locale, { style: 'unit', unit, unitDisplay: 'short', maximumFractionDigits: value < 10 ? 1 : 0 }).format(value)
}

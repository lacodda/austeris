// The interface's strings, as the application loads them: tests find
// controls by the words a person reads, in the source language.
import '@/i18n'

// jsdom lays nothing out, so it has no scrolling to do; the keyboard's
// cursor brings the row it lands on into view, which here is a no-op rather
// than a missing method.
if (typeof Element.prototype.scrollIntoView !== 'function') {
  Element.prototype.scrollIntoView = () => {}
}

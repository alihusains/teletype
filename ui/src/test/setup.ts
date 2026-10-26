import '@testing-library/jest-dom'

// jsdom has no layout engine, so this scroll helper is missing entirely.
// Screens call it when a form opens; without the stub the effect throws.
if (!Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = () => {}
}

import '@testing-library/jest-dom'
import { afterEach } from 'vitest'
import { unmountAllPillRoots } from '../dev/pillRootRegistry'

// jsdom has no layout engine, so this scroll helper is missing entirely.
// Screens call it when a form opens; without the stub the effect throws.
if (!Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = () => {}
}

// Tests that drive the pill through its real entry point leave a React root
// that testing-library's auto-cleanup cannot reach, and the recording phase's
// live timer then fires into a torn-down jsdom. See dev/pillRootRegistry.ts.
afterEach(() => {
  unmountAllPillRoots()
})

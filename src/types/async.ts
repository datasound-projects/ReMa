/** State of a value loaded asynchronously from the backend. */
export type AsyncState<T, E = Error> =
  | { status: 'loading' }
  | { status: 'success'; data: T }
  | { status: 'error'; error: E };

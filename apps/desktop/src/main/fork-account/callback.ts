/** Validate the callback against the exact attempt before consuming Clerk's nonce. */
export function accountCallback(actualURL: string, expectedURL: string): URL {
  const actual = new URL(actualURL)
  const expected = new URL(expectedURL)
  const attempt = expected.searchParams.get('attempt')
  if (!attempt || expected.searchParams.getAll('attempt').length !== 1 || actual.protocol !== expected.protocol ||
      actual.host !== expected.host || actual.pathname !== expected.pathname || actual.username || actual.password || actual.hash ||
      actual.searchParams.getAll('attempt').length !== 1 || actual.searchParams.get('attempt') !== attempt ||
      actual.searchParams.getAll('rotating_token_nonce').length > 1) throw new Error('Account callback does not match this attempt.')
  return actual
}

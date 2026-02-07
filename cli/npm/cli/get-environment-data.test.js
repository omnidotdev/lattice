const { getEnvironmentData } = require('./get-environment-data')

const originalPlatform = process.platform
const originalArch = process.arch

afterEach(() => {
  Object.defineProperty(process, 'platform', {
    value: originalPlatform,
  })
  Object.defineProperty(process, 'arch', {
    value: originalArch,
  })
})

test('aarch64-apple-darwin', () => {
  Object.defineProperty(process, 'platform', {
    value: 'darwin',
  })
  Object.defineProperty(process, 'arch', {
    value: 'arm64',
  })

  const environmentData = getEnvironmentData('lattice')

  expect(environmentData.targetTriple).toBe('aarch64-apple-darwin')
  expect(environmentData.binary).toBe('lattice')
})

test('x86_64-apple-darwin', () => {
  Object.defineProperty(process, 'platform', {
    value: 'darwin',
  })
  Object.defineProperty(process, 'arch', {
    value: 'x64',
  })

  const environmentData = getEnvironmentData('lattice')

  expect(environmentData.targetTriple).toBe('x86_64-apple-darwin')
  expect(environmentData.binary).toBe('lattice')
})

test('x86_64-unknown-linux-musl', () => {
  Object.defineProperty(process, 'platform', {
    value: 'linux',
  })
  Object.defineProperty(process, 'arch', {
    value: 'x64',
  })

  const environmentData = getEnvironmentData('lattice')

  expect(environmentData.targetTriple).toBe('x86_64-unknown-linux-musl')
  expect(environmentData.binary).toBe('lattice')
})

test('aarch64-unknown-linux-musl', () => {
  Object.defineProperty(process, 'platform', {
    value: 'linux',
  })
  Object.defineProperty(process, 'arch', {
    value: 'arm64',
  })

  const environmentData = getEnvironmentData('lattice')

  expect(environmentData.targetTriple).toBe('aarch64-unknown-linux-musl')
  expect(environmentData.binary).toBe('lattice')
})

test('x86_64-pc-windows-msvc', () => {
  Object.defineProperty(process, 'platform', {
    value: 'win32',
  })
  Object.defineProperty(process, 'arch', {
    value: 'x64',
  })

  const environmentData = getEnvironmentData('lattice')

  expect(environmentData.targetTriple).toBe('x86_64-pc-windows-msvc')
  expect(environmentData.binary).toBe('lattice.exe')
})

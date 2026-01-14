import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    globals: true,
    environment: 'node',
    include: ['tests/**/*.test.ts'],
    testTimeout: 30000,
    hookTimeout: 30000,
    // Integration tests are sequential by default
    sequence: {
      shuffle: false,
    },
    // Reporter for clear test output
    reporters: ['verbose'],
  },
});

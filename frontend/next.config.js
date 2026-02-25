/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  webpack: (config) => {
    config.resolve.fallback = { fs: false, net: false, tls: false };
    config.externals.push("pino-pretty", "lokijs", "encoding");
    // Stub react-native-async-storage for MetaMask SDK (web only)
    config.resolve.alias = {
      ...config.resolve.alias,
      "@react-native-async-storage/async-storage":
        require.resolve("./src/lib/async-storage-stub.js"),
    };
    return config;
  },
};

module.exports = nextConfig;

/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ["./src/**/*.{js,ts,jsx,tsx,mdx}"],
  theme: {
    extend: {
      colors: {
        bg: {
          primary: "#0d1117",
          secondary: "#161b22",
          tertiary: "#1c2128",
          hover: "#21262d",
        },
        border: {
          primary: "#30363d",
          secondary: "#21262d",
        },
        text: {
          primary: "#e6edf3",
          secondary: "#8b949e",
          tertiary: "#6e7681",
        },
        green: { DEFAULT: "#3fb950", dark: "#238636" },
        red: { DEFAULT: "#f85149", dark: "#da3633" },
        accent: "#58a6ff",
      },
      fontFamily: {
        mono: [
          "JetBrains Mono",
          "SF Mono",
          "Monaco",
          "Inconsolata",
          "Fira Mono",
          "Droid Sans Mono",
          "Source Code Pro",
          "monospace",
        ],
      },
    },
  },
  plugins: [],
};

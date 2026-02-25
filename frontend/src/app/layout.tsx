import type { Metadata } from "next";
import { Providers } from "./providers";
import "./globals.css";

export const metadata: Metadata = {
  title: "HyperCore Trading",
  description: "Spot trading terminal",
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en" className="dark">
      <body className="font-mono bg-bg-primary text-text-primary min-h-screen antialiased">
        <Providers>{children}</Providers>
      </body>
    </html>
  );
}

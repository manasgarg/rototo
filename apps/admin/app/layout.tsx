import type { Metadata } from "next";
import { Hanken_Grotesk, JetBrains_Mono, Manrope } from "next/font/google";
import "./styles.css";

const display = Manrope({ subsets: ["latin"], variable: "--font-display" });
const body = Hanken_Grotesk({ subsets: ["latin"], variable: "--font-body" });
const mono = JetBrains_Mono({ subsets: ["latin"], variable: "--font-mono" });

export const metadata: Metadata = {
  title: "rototo admin",
  description: "Review and publish rototo workspace configuration from GitHub.",
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html className={`${display.variable} ${body.variable} ${mono.variable}`} lang="en">
      <body>{children}</body>
    </html>
  );
}

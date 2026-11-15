/** @vitest-environment jsdom */
import { readFileSync } from "node:fs";
import { expect, it } from "vitest";

it.each([
  ["colored halo", /\.quota-widget-cloud::before\s*\{([^}]+)\}/],
  ["paper falloff", /\.quota-widget-backdrop\s*\{([^}]+)\}/],
])("keeps the %s gradient valid after resolving its color variables", (_, selector) => {
  const css = readFileSync("src/widget.css", "utf8");
  const rule = css.match(selector)?.[1];
  const gradient = rule?.match(/background:\s*([^;]+);/)?.[1];
  expect(gradient).toBeDefined();
  // Invalid gradients containing var() may survive stylesheet parsing but
  // disappear when computed. Validate the resolved value, not only the rule.
  const resolved = gradient.replaceAll("var(--coverage-core)", "#eee0b4")
    .replaceAll("var(--widget-paper)", "#fdfefe")
    .replace(/var\(--[\w-]+,\s*([^()]+)\)/g, "$1");
  expect(resolved).not.toContain("var(");
  const style = document.createElement("div").style;
  style.backgroundImage = resolved;
  expect(style.backgroundImage).toContain("radial-gradient(");
  expect(style.backgroundImage).toContain("color-mix(");
});

export interface QuotaColors {
  a: string;
  bridge: string;
  b: string;
  darkA: string;
  darkBridge: string;
  darkB: string;
}

const keys = ["a", "bridge", "b", "darkA", "darkBridge", "darkB"] as const;

// Coral → cream → clear cyan → mist blue. The cyan stop keeps the upper
// range luminous instead of letting complementary blue/yellow turn gray.
// OKLab interpolation and eased joins keep every stop continuous.
const depleted: QuotaColors = { a: "#e8bdb8", bridge: "#f7e0dc", b: "#fff7f5", darkA: "#98646c", darkBridge: "#704953", darkB: "#392c32" };
const middle: QuotaColors = { a: "#eee0b4", bridge: "#f8f0d9", b: "#fffcf3", darkA: "#897841", darkBridge: "#65573b", darkB: "#343026" };
const full: QuotaColors = { a: "#b9d9fb", bridge: "#dfedfd", b: "#f4f8ff", darkA: "#3f6b9b", darkBridge: "#314a65", darkB: "#28313c" };
const clear: QuotaColors = { a: "#b9dfe9", bridge: "#e0f0f5", b: "#f5fbff", darkA: "#4f7188", darkBridge: "#314f61", darkB: "#28333c" };

function toLab(hex: string): number[] {
  const rgb = [1, 3, 5].map((offset) => parseInt(hex.slice(offset, offset + 2), 16) / 255)
    .map((x) => x <= .04045 ? x / 12.92 : ((x + .055) / 1.055) ** 2.4);
  const l = Math.cbrt(.4122214708 * rgb[0] + .5363325363 * rgb[1] + .0514459929 * rgb[2]);
  const m = Math.cbrt(.2119034982 * rgb[0] + .6806995451 * rgb[1] + .1073969566 * rgb[2]);
  const s = Math.cbrt(.0883024619 * rgb[0] + .2817188376 * rgb[1] + .6299787005 * rgb[2]);
  return [.2104542553 * l + .793617785 * m - .0040720468 * s, 1.9779984951 * l - 2.428592205 * m + .4505937099 * s, .0259040371 * l + .7827717662 * m - .808675766 * s];
}

function fromLab([L, a, b]: number[]): string {
  const l = (L + .3963377774 * a + .2158037573 * b) ** 3;
  const m = (L - .1055613458 * a - .0638541728 * b) ** 3;
  const s = (L - .0894841775 * a - 1.291485548 * b) ** 3;
  return "#" + [4.0767416621 * l - 3.3077115913 * m + .2309699292 * s, -1.2684380046 * l + 2.6097574011 * m - .3413193965 * s, -.0041960863 * l - .7034186147 * m + 1.707614701 * s]
    .map((x) => Math.round(255 * Math.max(0, Math.min(1, x <= .0031308 ? 12.92 * x : 1.055 * x ** (1 / 2.4) - .055))))
    .map((x) => x.toString(16).padStart(2, "0")).join("");
}

function mix(left: string, right: string, amount: number): string {
  const a = toLab(left), b = toLab(right);
  return fromLab(a.map((x, index) => x + (b[index] - x) * amount));
}

// The more saturated dark amber needs a flatter transition to cyan. Ramp
// smoothly at both ends; a slight bias toward amber offsets the steeper
// blue-channel response near that end of the OKLab path. Both joins stay flat.
function gentleWarmTransition(t: number): number {
  const ramp = 1 / 12;
  if (t < ramp) return t * t / (2 * ramp * (1 - ramp));
  if (t > 1 - ramp) return 1 - (1 - t) ** 2 / (2 * ramp * (1 - ramp));
  return (t - ramp / 2) / (1 - ramp);
}

export function interpolateQuotaColors(from: QuotaColors, to: QuotaColors, amount: number): QuotaColors {
  const t = Math.max(0, Math.min(1, amount));
  return Object.fromEntries(keys.map((key) => [key, mix(from[key], to[key], t)])) as unknown as QuotaColors;
}

export function getQuotaColors(percent: number | null): QuotaColors {
  if (percent === null || !Number.isFinite(percent)) {
    return { a: "#d7e0e7", bridge: "#edf1f4", b: "#f9fafb", darkA: "#4d5b67", darkBridge: "#37444f", darkB: "#2c343c" };
  }
  const value = Math.max(0, Math.min(100, percent));
  const t = value <= 50 ? value / 50 : value <= 80 ? (value - 50) / 30 : (value - 80) / 20;
  const eased = t * t * (3 - 2 * t);
  return value <= 50
    ? interpolateQuotaColors(depleted, middle, eased)
    : value <= 80
      ? { ...interpolateQuotaColors(middle, clear, eased),
          darkA: mix(middle.darkA, clear.darkA, gentleWarmTransition(t) ** 1.15) }
      : interpolateQuotaColors(clear, full, eased);
}

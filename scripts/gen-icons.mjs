// Generates the app icon set with no external dependencies.
//   node scripts/gen-icons.mjs
// Output: src-tauri/icons/{32x32,128x128,128x128@2x,icon}.png, icon.ico, icon.icns
import { deflateSync } from "node:zlib";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const OUT = resolve(dirname(fileURLToPath(import.meta.url)), "../src-tauri/icons");
mkdirSync(OUT, { recursive: true });

const BG = [0x14, 0x16, 0x1a, 0xff];
const FG = [0xe8, 0x66, 0x3d, 0xff];
const FG2 = [0xf4, 0xf4, 0xf5, 0xff];

/** Draws the logo: rounded square + ">" chevron + cursor. */
function render(size) {
  const px = new Uint8Array(size * size * 4);
  const r = size * 0.22; // corner radius
  const put = (x, y, c) => {
    const i = (y * size + x) * 4;
    px[i] = c[0];
    px[i + 1] = c[1];
    px[i + 2] = c[2];
    px[i + 3] = c[3];
  };
  const inRounded = (x, y) => {
    const cx = Math.min(Math.max(x, r), size - r);
    const cy = Math.min(Math.max(y, r), size - r);
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r + 0.5;
  };

  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      put(x, y, inRounded(x + 0.5, y + 0.5) ? BG : [0, 0, 0, 0]);
    }
  }

  // ">" chevron on the left.
  const w = Math.max(2, Math.round(size * 0.075)); // stroke width
  const x0 = Math.round(size * 0.24);
  const y0 = Math.round(size * 0.3);
  const y1 = Math.round(size * 0.7);
  const xm = Math.round(size * 0.44);
  const line = (ax, ay, bx, by, color) => {
    const steps = Math.round(Math.hypot(bx - ax, by - ay) * 2);
    for (let s = 0; s <= steps; s++) {
      const t = s / steps;
      const cx = ax + (bx - ax) * t;
      const cy = ay + (by - ay) * t;
      for (let dy = -w / 2; dy <= w / 2; dy += 0.5) {
        for (let dx = -w / 2; dx <= w / 2; dx += 0.5) {
          const x = Math.round(cx + dx);
          const y = Math.round(cy + dy);
          if (x >= 0 && y >= 0 && x < size && y < size) put(x, y, color);
        }
      }
    }
  };
  line(x0, y0, xm, size / 2, FG);
  line(xm, size / 2, x0, y1, FG);

  // Cursor: block on the right.
  const bx0 = Math.round(size * 0.55);
  const bx1 = Math.round(size * 0.76);
  const by0 = Math.round(size * 0.62);
  const by1 = Math.round(size * 0.7);
  for (let y = by0; y < by1; y++) for (let x = bx0; x < bx1; x++) put(x, y, FG2);

  return px;
}

const crcTable = (() => {
  const t = new Int32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c;
  }
  return t;
})();
const crc32 = (buf) => {
  let c = -1;
  for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ -1) >>> 0;
};

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}

function png(size) {
  const px = render(size);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  const raw = Buffer.alloc(size * (size * 4 + 1));
  for (let y = 0; y < size; y++) {
    raw[y * (size * 4 + 1)] = 0; // filter: none
    Buffer.from(px.buffer, y * size * 4, size * 4).copy(raw, y * (size * 4 + 1) + 1);
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

/** ICO with embedded PNGs (supported by Windows Vista+). */
function ico(sizes) {
  const imgs = sizes.map((s) => ({ s, buf: png(s) }));
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2); // icon type
  header.writeUInt16LE(imgs.length, 4);
  let offset = 6 + imgs.length * 16;
  const dir = [];
  for (const { s, buf } of imgs) {
    const e = Buffer.alloc(16);
    e[0] = s >= 256 ? 0 : s;
    e[1] = s >= 256 ? 0 : s;
    e.writeUInt16LE(1, 4); // planes
    e.writeUInt16LE(32, 6); // bpp
    e.writeUInt32BE(buf.length, 8);
    e.writeUInt32LE(buf.length, 8);
    e.writeUInt32LE(offset, 12);
    dir.push(e);
    offset += buf.length;
  }
  return Buffer.concat([header, ...dir, ...imgs.map((i) => i.buf)]);
}

/** ICNS with PNG entries (ic07/ic08/ic09/ic10). */
function icns() {
  const kinds = [
    ["ic07", 128],
    ["ic08", 256],
    ["ic09", 512],
    ["ic10", 1024],
  ];
  const parts = kinds.map(([t, s]) => {
    const data = png(s);
    const head = Buffer.alloc(8);
    head.write(t, 0, "ascii");
    head.writeUInt32BE(data.length + 8, 4);
    return Buffer.concat([head, data]);
  });
  const body = Buffer.concat(parts);
  const head = Buffer.alloc(8);
  head.write("icns", 0, "ascii");
  head.writeUInt32BE(body.length + 8, 4);
  return Buffer.concat([head, body]);
}

const outputs = {
  "32x32.png": png(32),
  "128x128.png": png(128),
  "128x128@2x.png": png(256),
  "icon.png": png(512),
  "Square30x30Logo.png": png(30),
  "Square44x44Logo.png": png(44),
  "Square71x71Logo.png": png(71),
  "Square89x89Logo.png": png(89),
  "Square107x107Logo.png": png(107),
  "Square142x142Logo.png": png(142),
  "Square150x150Logo.png": png(150),
  "Square284x284Logo.png": png(284),
  "Square310x310Logo.png": png(310),
  "StoreLogo.png": png(50),
  "icon.ico": ico([16, 32, 48, 64, 128, 256]),
  "icon.icns": icns(),
};

for (const [name, buf] of Object.entries(outputs)) {
  writeFileSync(resolve(OUT, name), buf);
}
console.log(`${Object.keys(outputs).length} iconos escritos en ${OUT}`);

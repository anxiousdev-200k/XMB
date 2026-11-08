// Documents and comics shown page by page in the Photo Viewer (custom
// sections): PDF (pdf.js), CBZ (zip, fflate) and CBT (tar). Each page becomes
// an image URL; pages are made as needed and kept in a bounded document cache.
//   pdf.js © Mozilla (Apache-2.0), fflate © Arjun Barrett (MIT): src/vendor

import { cmpNatural } from "./api.js";

export const DOC_RE = /\.(pdf|cbz|cbt)$/i;
export const isDocPath = (p) => DOC_RE.test(String(p || ""));
const IMG_RE = /\.(jpe?g|png|gif|webp|bmp|avif)$/i;
const natural = cmpNatural;
const mime = (n) => /\.png$/i.test(n) ? "image/png" : /\.gif$/i.test(n) ? "image/gif" : /\.webp$/i.test(n) ? "image/webp"
  : /\.bmp$/i.test(n) ? "image/bmp" : /\.avif$/i.test(n) ? "image/avif" : "image/jpeg";

const docs = new Map();      // url -> { promise, refs }
const MAX_DOCS = 4;
const MAX_PAGES = 5;
const MAX_THUMBS = 256;

function revokePage(entry) {
  if (!entry.url) return;
  URL.revokeObjectURL(entry.url);
  entry.url = null;
}
function trimPages(pages) {
  while (pages.size > MAX_PAGES) {
    const [i, entry] = pages.entries().next().value;
    pages.delete(i);
    revokePage(entry);
  }
}
function trimDocs() {
  while (docs.size > MAX_DOCS) {
    const candidate = [...docs].find(([, entry]) => entry.refs === 0);
    if (!candidate) return;
    const [url, entry] = candidate;
    docs.delete(url);
    entry.promise.then((doc) => doc.dispose()).catch((error) => console.warn("document cache cleanup", error));
  }
}

let _pdfjs = null;
async function pdfjs() {
  if (!_pdfjs) {
    _pdfjs = import("../vendor/pdf.min.mjs").then((m) => {
      m.GlobalWorkerOptions.workerSrc = new URL("../vendor/pdf.worker.min.mjs", import.meta.url).href;
      return m;
    });
  }
  return _pdfjs;
}

async function openPdf(url) {
  const lib = await pdfjs();
  const doc = await lib.getDocument({ url, isEvalSupported: false }).promise;
  const made = new Map();
  let disposed = false;
  return {
    count: doc.numPages,
    page(i) {
      if (disposed || i < 0 || i >= doc.numPages) return Promise.reject(new Error("document page is unavailable"));
      let entry = made.get(i);
      if (!entry) {
        entry = { url: null, promise: null };
        made.set(i, entry);
        entry.promise = (async () => {
          const pg = await doc.getPage(i + 1);
          const v1 = pg.getViewport({ scale: 1 });
          const k = Math.min(3, 2200 / Math.max(v1.width, v1.height));
          const vp = pg.getViewport({ scale: k });
          const c = document.createElement("canvas");
          c.width = Math.ceil(vp.width); c.height = Math.ceil(vp.height);
          const ctx = c.getContext("2d");
          ctx.fillStyle = "#fff"; ctx.fillRect(0, 0, c.width, c.height);
          await pg.render({ canvasContext: ctx, viewport: vp }).promise;
          const blob = await new Promise((resolve) => c.toBlob(resolve, "image/jpeg", 0.9));
          const pageUrl = URL.createObjectURL(blob);
          if (disposed || made.get(i) !== entry) {
            URL.revokeObjectURL(pageUrl);
            throw new Error("document page is no longer needed");
          }
          entry.url = pageUrl;
          return pageUrl;
        })();
        trimPages(made);
      } else {
        made.delete(i); made.set(i, entry);
      }
      return entry.promise;
    },
    async dispose() {
      disposed = true;
      for (const entry of made.values()) revokePage(entry);
      made.clear();
      await doc.destroy();
    },
  };
}

// a comic: the pictures in the archive, in name order
function fromEntries(entries) {
  entries.sort((a, b) => natural(a.name, b.name));
  const urls = new Map();
  let disposed = false;
  return {
    count: entries.length,
    page(i) {
      if (disposed) return Promise.reject(new Error("document is closed"));
      const e = entries[i];
      if (!e) return Promise.reject(new Error("no page"));
      let entry = urls.get(i);
      if (entry) {
        urls.delete(i); urls.set(i, entry);
        return Promise.resolve(entry.url);
      }
      entry = { url: URL.createObjectURL(new Blob([e.data], { type: mime(e.name) })) };
      urls.set(i, entry);
      trimPages(urls);
      return Promise.resolve(entry.url);
    },
    dispose() {
      disposed = true;
      for (const entry of urls.values()) revokePage(entry);
      urls.clear();
      entries.length = 0;
    },
  };
}
async function openCbz(url) {
  const { unzipSync } = await import("../vendor/fflate.mjs");
  const buf = new Uint8Array(await (await fetch(url)).arrayBuffer());
  const files = unzipSync(buf, { filter: (f) => IMG_RE.test(f.name) && !/(^|\/)(__MACOSX|\.)/.test(f.name) });
  return fromEntries(Object.entries(files).map(([name, data]) => ({ name, data })));
}
async function openCbt(url) {
  const buf = new Uint8Array(await (await fetch(url)).arrayBuffer());
  const out = [];
  const str = (o, n) => { let s = ""; for (let i = o; i < o + n && buf[i]; i++) s += String.fromCharCode(buf[i]); return s; };
  let off = 0;
  while (off + 512 <= buf.length) {
    const name = str(off, 100);
    if (!name) break;
    const size = parseInt(str(off + 124, 12).trim() || "0", 8) || 0;
    const type = String.fromCharCode(buf[off + 156] || 48);
    const prefix = str(off + 345, 155);
    const full = prefix ? `${prefix}/${name}` : name;
    if ((type === "0" || type === "\0") && IMG_RE.test(full)) out.push({ name: full, data: buf.slice(off + 512, off + 512 + size) });
    off += 512 + Math.ceil(size / 512) * 512;
  }
  return fromEntries(out);
}

function docEntry(url, path) {
  if (!docs.has(url)) {
    const p = String(path || url);
    const pr = /\.pdf$/i.test(p) ? openPdf(url) : /\.cbt$/i.test(p) ? openCbt(url) : openCbz(url);
    const entry = { promise: pr, refs: 0 };
    pr.catch(() => { if (docs.get(url) === entry) docs.delete(url); });
    docs.set(url, entry);
  }
  const entry = docs.get(url);
  docs.delete(url); docs.set(url, entry);
  return entry;
}
export function openDoc(url, path) {
  const entry = docEntry(url, path);
  trimDocs();
  return entry.promise;
}
export function retainDoc(url, path) {
  const entry = docEntry(url, path);
  entry.refs++;
  return entry.promise.catch((error) => {
    if (docs.get(url) === entry) { entry.refs--; trimDocs(); }
    throw error;
  });
}
export function releaseDoc(url) {
  const entry = docs.get(url);
  if (!entry || !entry.refs) return;
  entry.refs--;
  trimDocs();
}

// the first page, small, for the item's icon
const thumbs = new Map();
export function docThumb(url, path) {
  if (!thumbs.has(url)) {
    const promise = (async () => {
      let retained = false;
      try {
        const d = await retainDoc(url, path);
        retained = true;
        const src = await d.page(0);
        const img = new Image(); img.src = src; await img.decode();
        const k = Math.min(1, 480 / Math.max(img.naturalWidth, img.naturalHeight));
        const c = document.createElement("canvas");
        c.width = Math.max(1, Math.round(img.naturalWidth * k)); c.height = Math.max(1, Math.round(img.naturalHeight * k));
        c.getContext("2d").drawImage(img, 0, 0, c.width, c.height);
        return c.toDataURL("image/jpeg", 0.85);
      } finally {
        if (retained) releaseDoc(url);
      }
    })();
    thumbs.set(url, promise);
    promise.catch(() => { if (thumbs.get(url) === promise) thumbs.delete(url); });
  }
  const promise = thumbs.get(url);
  thumbs.delete(url); thumbs.set(url, promise);
  while (thumbs.size > MAX_THUMBS) thumbs.delete(thumbs.keys().next().value);
  return promise;
}

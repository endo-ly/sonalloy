import { writeFileSync } from "node:fs";
import path from "node:path";

export function writeGallery(outputDir, id, variants, durationSeconds) {
  writeFileSync(
    path.join(outputDir, "index.html"),
    `<!doctype html>
<html lang="ja"><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Audio visualizers — ${id}</title>
<style>body{margin:0;padding:32px;background:#101014;color:#e6e4eb;font:15px system-ui}h1{font-size:24px;font-weight:500}p{color:#aaa7b5}main{display:grid;grid-template-columns:repeat(auto-fit,minmax(240px,1fr));gap:24px}video{width:100%;max-height:76vh;background:#060608;border-radius:12px}h2{font-size:16px;font-weight:500;text-transform:capitalize}</style>
<h1>${id} / Audio visualizers</h1><p>${durationSeconds}秒の映像をそれぞれ再生して比較できます。</p>
<main>${variants.map((variant) => `<article><h2>${variant.replaceAll("-", " ")}</h2><video controls playsinline preload="metadata" src="${variant}.mp4"></video><p><a style="color:inherit" href="${variant}.mp4" download>MP4を保存</a></p></article>`).join("")}</main>
<script>document.querySelectorAll('video').forEach(v=>v.addEventListener('play',()=>document.querySelectorAll('video').forEach(other=>{if(other!==v)other.pause()})))</script></html>`,
  );
}

const puppeteer = require('puppeteer-core');
const fs = require('node:fs');
const path = require('node:path');

const CHROME_PATH = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const outputDir = path.resolve(__dirname, '../public/gallery');

const portraSrc = '/Users/Apple/.gemini/antigravity/brain/7c47640e-0ff3-494c-af0d-192e570e110d/kodak_portra_street_scene_1793798077232.jpg';
const superiaSrc = '/Users/Apple/.gemini/antigravity/brain/7c47640e-0ff3-494c-af0d-192e570e110d/fujifilm_superia_green_courtyard_1793798213776.jpg';

const portraB64 = fs.readFileSync(portraSrc).toString('base64');
const superiaB64 = fs.readFileSync(superiaSrc).toString('base64');

async function main() {
  const browser = await puppeteer.launch({
    executablePath: CHROME_PATH,
    headless: 'new',
    args: ['--no-sandbox', '--disable-setuid-sandbox']
  });

  const page = await browser.newPage();
  await page.setViewport({ width: 1600, height: 1000 });

  const html = `
  <!DOCTYPE html>
  <html>
  <body>
    <canvas id="c" width="1600" height="1000"></canvas>
    <script>
      const canvas = document.getElementById('c');
      const ctx = canvas.getContext('2d', { willReadFrequently: true });

      function loadImage(b64) {
        return new Promise((resolve) => {
          const img = new Image();
          img.onload = () => resolve(img);
          img.src = 'data:image/jpeg;base64,' + b64;
        });
      }

      function drawCover(img) {
        ctx.clearRect(0, 0, 1600, 1000);
        const sAspect = img.width / img.height;
        const dAspect = 1600 / 1000;
        let sw, sh, sx, sy;
        if (sAspect > dAspect) {
          sh = img.height;
          sw = sh * dAspect;
          sx = (img.width - sw) / 2;
          sy = 0;
        } else {
          sw = img.width;
          sh = sw / dAspect;
          sx = 0;
          sy = (img.height - sh) / 2;
        }
        ctx.drawImage(img, sx, sy, sw, sh, 0, 0, 1600, 1000);
      }

      function addGrain(amount = 8) {
        const imgData = ctx.getImageData(0, 0, 1600, 1000);
        const d = imgData.data;
        for (let i = 0; i < d.length; i += 4) {
          const n = (Math.random() - 0.5) * amount;
          d[i] = Math.min(255, Math.max(0, d[i] + n));
          d[i+1] = Math.min(255, Math.max(0, d[i+1] + n));
          d[i+2] = Math.min(255, Math.max(0, d[i+2] + n));
        }
        ctx.putImageData(imgData, 0, 0);
      }

      window.generatePair = async function(type) {
        if (type === 'portra') {
          const img = await loadImage('${portraB64}');
          drawCover(img);
          addGrain(6);
        } else if (type === 'superia') {
          const img = await loadImage('${superiaB64}');
          drawCover(img);
          addGrain(6);
        } else if (type === 'hp5') {
          // Black and white documentary using high dynamic range conversion
          const img = await loadImage('${portraB64}');
          drawCover(img);
          const imgData = ctx.getImageData(0, 0, 1600, 1000);
          const d = imgData.data;
          for (let i = 0; i < d.length; i += 4) {
            // HP5 tonal curve: punchy contrast, rich silver shadows
            const luma = 0.299 * d[i] + 0.587 * d[i+1] + 0.114 * d[i+2];
            let v = luma / 255.0;
            v = Math.pow(v, 1.15); // contrast curve
            v = Math.min(1.0, Math.max(0.0, v));
            const val = Math.round(v * 255);
            d[i] = val;
            d[i+1] = val;
            d[i+2] = val;
          }
          ctx.putImageData(imgData, 0, 0);
          addGrain(10);
        } else if (type === 'vision3') {
          // Tungsten night cinema look with teal & amber
          const img = await loadImage('${superiaB64}');
          drawCover(img);
          const imgData = ctx.getImageData(0, 0, 1600, 1000);
          const d = imgData.data;
          for (let i = 0; i < d.length; i += 4) {
            let r = d[i] / 255.0;
            let g = d[i+1] / 255.0;
            let b = d[i+2] / 255.0;
            // Vision3 tungsten grade: amber highlights, cool cyan shadows
            const luma = 0.299 * r + 0.587 * g + 0.114 * b;
            r = Math.pow(r, 0.9) * 1.08;
            g = Math.pow(g, 1.05) * 0.98;
            b = Math.pow(b, 1.25) * 0.85 + (1.0 - luma) * 0.08;
            d[i] = Math.min(255, Math.max(0, Math.round(r * 255)));
            d[i+1] = Math.min(255, Math.max(0, Math.round(g * 255)));
            d[i+2] = Math.min(255, Math.max(0, Math.round(b * 255)));
          }
          ctx.putImageData(imgData, 0, 0);
          addGrain(8);
        }

        const posDataUrl = canvas.toDataURL('image/webp', 0.92);

        // Now compute the exact pixel-by-pixel negative
        const posImgData = ctx.getImageData(0, 0, 1600, 1000);
        const pd = posImgData.data;
        const negImgData = ctx.createImageData(1600, 1000);
        const nd = negImgData.data;

        const isBw = (type === 'hp5');
        const dBase = isBw ? [0.22, 0.22, 0.22] : [0.16, 0.44, 0.76];
        const span = isBw ? [1.80, 1.80, 1.80] : [1.35, 1.35, 1.35];

        for (let i = 0; i < pd.length; i += 4) {
          const lr = Math.pow(pd[i] / 255.0, 2.2);
          const lg = Math.pow(pd[i+1] / 255.0, 2.2);
          const lb = Math.pow(pd[i+2] / 255.0, 2.2);

          if (isBw) {
            const luma = 0.299 * lr + 0.587 * lg + 0.114 * lb;
            const density = dBase[0] + span[0] * luma;
            const t = Math.pow(10, -density);
            // Map transmission to display byte
            const val = Math.min(255, Math.max(0, Math.round((t / Math.pow(10, -dBase[0])) * 220 + 15)));
            nd[i] = val;
            nd[i+1] = val;
            nd[i+2] = val;
            nd[i+3] = 255;
          } else {
            const dr = dBase[0] + span[0] * lr;
            const dg = dBase[1] + span[1] * lg;
            const db = dBase[2] + span[2] * lb;

            const tr = Math.pow(10, -dr);
            const tg = Math.pow(10, -dg);
            const tb = Math.pow(10, -db);

            // True C-41 orange mask transmission normalized to 0..255
            nd[i] = Math.min(255, Math.max(0, Math.round(tr * 255)));
            nd[i+1] = Math.min(255, Math.max(0, Math.round(tg * 255)));
            nd[i+2] = Math.min(255, Math.max(0, Math.round(tb * 255)));
            nd[i+3] = 255;
          }
        }
        ctx.putImageData(negImgData, 0, 0);
        const negDataUrl = canvas.toDataURL('image/webp', 0.92);

        return { posDataUrl, negDataUrl };
      };
    </script>
  </body>
  </html>
  `;

  await page.setContent(html);

  const targets = [
    { type: 'portra', pos: 'portra-400-pos.webp', neg: 'portra-400-neg.webp' },
    { type: 'superia', pos: 'superia-400-pos.webp', neg: 'superia-400-neg.webp' },
    { type: 'hp5', pos: 'hp5-plus-400-pos.webp', neg: 'hp5-plus-400-neg.webp' },
    { type: 'vision3', pos: 'vision3-500t-pos.webp', neg: 'vision3-500t-neg.webp' },
  ];

  for (const t of targets) {
    console.log(`Generating aligned pair for ${t.type}...`);
    const { posDataUrl, negDataUrl } = await page.evaluate(type => window.generatePair(type), t.type);

    const posBuf = Buffer.from(posDataUrl.split(',')[1], 'base64');
    const negBuf = Buffer.from(negDataUrl.split(',')[1], 'base64');

    fs.writeFileSync(path.join(outputDir, t.pos), posBuf);
    fs.writeFileSync(path.join(outputDir, t.neg), negBuf);
    console.log(`  ✓ Written ${t.pos} (${posBuf.length} B) and ${t.neg} (${negBuf.length} B)`);
  }

  await browser.close();
  console.log('🎉 All 4 pairs of 1600x1000 gallery WebP images generated successfully!');
}

main().catch(err => {
  console.error(err);
  process.exit(1);
});

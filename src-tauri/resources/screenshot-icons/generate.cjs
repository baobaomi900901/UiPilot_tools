const fs = require('fs');
const path = require('path');
// Development-only generator. Runtime embeds the generated alpha masks.
// Pass a locally installed sharp module path if sharp is not on NODE_PATH.
const sharp = require(process.argv[2] || 'sharp');
(async () => {
  for (const name of ['x', 'pin', 'save', 'copy']) {
    const source = fs.readFileSync(path.join(__dirname, '../../../node_modules/lucide-react/dist/esm/icons', `${name}.mjs`), 'utf8');
    const match = source.match(/const __iconNode = ([\s\S]*?);\r?\nconst /);
    if (!match) throw new Error(`Missing canonical Lucide node: ${name}`);
    const nodes = Function(`return (${match[1]})`)();
    const shapes = nodes.map(([tag, attrs]) => `<${tag} ${Object.entries(attrs).filter(([key]) => key !== 'key').map(([key, value]) => `${key}="${value}"`).join(' ')}/>`).join('');
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${shapes}</svg>`;
    const {data, info} = await sharp(Buffer.from(svg)).ensureAlpha().raw().toBuffer({resolveWithObject: true});
    const alpha = Buffer.alloc(64 * 64);
    for(let i = 0; i < alpha.length; i++) alpha[i] = data[i * info.channels + info.channels - 1];
    fs.writeFileSync(path.join(__dirname, `${name}.alpha`), alpha);
  }
})();

const hljs = require('highlight.js');
const fs = require('fs');
const files = new Set(fs.readdirSync('node_modules/@highlightjs/cdn-assets/languages').filter(f => f.endsWith('.min.js')).map(f => f.replace('.min.js','')));
const map = {};
for (const name of hljs.listLanguages()) {
  if (!files.has(name)) continue;
  map[name] = name;
  for (const a of (hljs.getLanguage(name).aliases || [])) map[a.toLowerCase()] = name;
}
fs.writeFileSync('../assets/vendor/hljs/aliases.json', JSON.stringify(map));
console.log(Object.keys(map).length + ' aliases for ' + files.size + ' grammars');

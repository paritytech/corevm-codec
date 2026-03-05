import fs from 'fs'

async function generateImportObject(code) {
    const module = await WebAssembly.compile(code)
    const rawImports = WebAssembly.Module.imports(module)
    const imports = new Map()
    for (const { module, name, ...rest } of rawImports) {
        const names = imports.get(module)
        if (names === undefined) {
            imports.set(module, [name])
        } else {
            names.push(name)
        }
    }
    for (const names of imports.values()) {
        names.sort()
    }
    let output = ''
    for (const [module, names] of imports.entries()) {
        output += `import {\n  ${names.join(',\n  ')}\n} from ${JSON.stringify(module)}\n`
    }
    output += `\n/** Second argument of \`WebAssembly.instantiate\`. */\nexport const IMPORT_OBJECT = Object.freeze({\n`
    for (const [module, names] of imports.entries()) {
        output += `  ${JSON.stringify(module)}: {\n`
        for (const name of names) {
            output += `    ${name},\n`
        }
        output += `  }\n`
    }
    output += `})\n`
    return output
}

async function patchPackageJson() {
    const package_json = JSON.parse(await fs.promises.readFile('pkg/package.json', 'utf-8'))
    const files = new Set(package_json.files)
    files.add('loader.js')
    files.add('code.wasm')
    files.delete('corevm_codec_bg.wasm')
    package_json.files = Array.from(files)
    await fs.promises.writeFile('pkg/package.json', JSON.stringify(package_json, null, 2))
}

async function patchIndexJs() {
    let text = await fs.promises.readFile('pkg/corevm_codec.js', 'utf-8')
    text = text.replaceAll('./corevm_codec_bg.wasm', './code.wasm')
    await fs.promises.writeFile('pkg/corevm_codec.js', text)
}

try {
    await fs.promises.rename('pkg/corevm_codec_bg.wasm', 'pkg/code.wasm')
} catch (e) {
    if (e.code !== 'ENOENT') {
        throw e
    }
}
const code = await fs.promises.readFile('pkg/code.wasm')
let output = ''
output += await fs.promises.readFile('js/loader.js', 'utf-8')
output += '\n'
output += await generateImportObject(code)
output += '\n\n'
output += `export * from './corevm_codec_bg.js'\n\n`
await fs.promises.writeFile('pkg/loader.js', output)
await patchPackageJson()
await patchIndexJs()

export default {
    compilerOptions: {
        lib: ['esnext'],
        allowJs: true,
    },
    highlightLanguages: ['rust', 'javascript', 'bash'],
    excludeNotDocumented: true,
    entryPoints: ['pkg/corevm_codec.d.ts', 'pkg/loader.js'],
}

import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// https://vitejs.dev/config/
export default defineConfig({
    plugins: [react()],
    // public/ is purely the build output (staged into the binary by build.rs),
    // not a Vite static-asset source; disable the public-dir copy feature.
    publicDir: false,
    server: {
        port: 3000,
        proxy: {
            '/api': {
                target: 'http://localhost:9000',
                changeOrigin: true,
                secure: false,
            },
        },
    },
    build: {
        outDir: './public',
        emptyOutDir: true,
    },
})

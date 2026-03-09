import { VideoDecoder } from './pkg/corevm_codec.js'
import fs from 'fs'
import assert from 'node:assert'

const input = fs.readFileSync('quake-frames.corevm')
const decoder = new VideoDecoder(input)
console.debug(`Video ${decoder.width}x${decoder.height}`)
assert.equal(decoder.width, 320)
assert.equal(decoder.height, 200)

#!/usr/bin/env node

import { runCli } from './cli.js'
import { validatePackage } from './source-validation.js'

const result = await runCli(process.argv.slice(2), process.platform, validatePackage)
process.stdout.write(result.stdout)
process.exitCode = result.exitCode

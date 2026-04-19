#!/usr/bin/env bun
import { $ } from "bun";
await $`ls *.ts | wc -l`
const output = await $`cat package.json`.json()
console.log(output);

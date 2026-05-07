#!/usr/bin/env node

import { Command } from 'commander';
import { showCommand } from './commands/show';
import { installCommand } from './commands/install';
import { exportCommand } from './commands/export';

const program = new Command();

program
  .name('setup')
  .description('CLI tool for managing Windows applications via winget')
  .version('1.0.0');

program
  .command('show')
  .description('Display all installed applications in a table')
  .action(async () => {
    await showCommand();
  });

program
  .command('install')
  .description('Install applications from config file')
  .argument('<path>', 'Path to config.json file')
  .action(async (configPath: string) => {
    await installCommand(configPath);
  });

program
  .command('export')
  .description('Export installed applications to config file')
  .argument('<path>', 'Output path for config.json')
  .action(async (outputPath: string) => {
    await exportCommand(outputPath);
  });

// Also support --config and --export flags for backward compatibility
program
  .option('-c, --config <path>', 'Install applications from config file')
  .option('-e, --export <path>', 'Export installed applications to config file');

async function main() {
  await program.parseAsync(process.argv);

  const options = program.opts();

  // Handle flag-style options if no command was run
  if (options.config) {
    await installCommand(options.config);
  } else if (options.export) {
    await exportCommand(options.export);
  }
}

main().catch((error) => {
  console.error('Error:', error.message);
  process.exit(1);
});

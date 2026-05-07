import fs from 'fs';
import path from 'path';
import chalk from 'chalk';
import Table from 'cli-table3';
import { Config } from '../types/config';
import { WingetService } from '../services/winget';

export async function installCommand(configPath: string): Promise<void> {
  const winget = new WingetService();

  // Resolve absolute path
  const absolutePath = path.resolve(configPath);

  console.log(chalk.cyan(`\n📂 Loading config from: ${absolutePath}\n`));

  // Check if file exists
  if (!fs.existsSync(absolutePath)) {
    console.error(chalk.red(`Error: Config file not found: ${absolutePath}`));
    process.exit(1);
  }

  // Read and parse config
  let config: Config;
  try {
    const content = fs.readFileSync(absolutePath, 'utf-8');
    config = JSON.parse(content);
  } catch (error: any) {
    console.error(chalk.red('Error parsing config file:'), error.message);
    process.exit(1);
  }

  if (!config.apps || !Array.isArray(config.apps)) {
    console.error(chalk.red('Error: Config must contain an "apps" array'));
    process.exit(1);
  }

  console.log(chalk.cyan(`Found ${config.apps.length} applications to install\n`));

  const results: { name: string; status: string; message: string }[] = [];

  for (const app of config.apps) {
    console.log(chalk.white(`\n▶ Installing: ${chalk.bold(app.name)} (${app.id})`));

    // Check if marked as unavailable in winget
    if (app.availableInWinget === false) {
      console.log(chalk.yellow(`  ⚠ Not available via winget`));
      if (app.note) {
        console.log(chalk.yellow(`  📝 ${app.note}`));
      }
      results.push({
        name: app.name,
        status: 'SKIPPED',
        message: app.note || 'Not available in winget'
      });
      continue;
    }

    // Install via winget
    const result = await winget.installApp(app.id);

    if (result.success) {
      console.log(chalk.green(`  ✓ ${result.message}`));
      results.push({
        name: app.name,
        status: 'OK',
        message: result.message
      });
    } else {
      console.log(chalk.red(`  ✗ ${result.message}`));
      results.push({
        name: app.name,
        status: 'FAILED',
        message: result.message.substring(0, 50)
      });
    }
  }

  // Print summary
  console.log(chalk.cyan('\n\n📊 Installation Summary\n'));

  const table = new Table({
    head: [
      chalk.bold.white('Application'),
      chalk.bold.white('Status'),
      chalk.bold.white('Message')
    ],
    style: {
      head: [],
      border: ['gray']
    },
    colWidths: [30, 12, 50]
  });

  for (const result of results) {
    let statusCell: string;
    switch (result.status) {
      case 'OK':
        statusCell = chalk.green('✓ OK');
        break;
      case 'SKIPPED':
        statusCell = chalk.yellow('⚠ SKIP');
        break;
      case 'FAILED':
        statusCell = chalk.red('✗ FAIL');
        break;
      default:
        statusCell = result.status;
    }

    table.push([
      result.name.substring(0, 28),
      statusCell,
      result.message.substring(0, 48)
    ]);
  }

  console.log(table.toString());

  const successful = results.filter(r => r.status === 'OK').length;
  const skipped = results.filter(r => r.status === 'SKIPPED').length;
  const failed = results.filter(r => r.status === 'FAILED').length;

  console.log(chalk.gray(`\nTotal: ${results.length} | `) +
    chalk.green(`Success: ${successful} | `) +
    chalk.yellow(`Skipped: ${skipped} | `) +
    chalk.red(`Failed: ${failed}\n`));
}

import fs from 'fs';
import path from 'path';
import chalk from 'chalk';
import { Config, AppConfig } from '../types/config';
import { WingetService } from '../services/winget';

export async function exportCommand(outputPath: string): Promise<void> {
  const winget = new WingetService();

  console.log(chalk.cyan('\n📦 Fetching installed applications...\n'));

  const allApps = winget.getInstalledApps();

  if (allApps.length === 0) {
    console.log(chalk.yellow('No applications found.'));
    return;
  }

  // Filter out system apps, drivers, and runtimes
  const apps = allApps.filter(app => !winget.isSystemApp(app));
  const excludedCount = allApps.length - apps.length;

  console.log(chalk.cyan(`Found ${allApps.length} installed applications`));
  console.log(chalk.gray(`  Excluded ${excludedCount} system apps/drivers/runtimes`));
  console.log(chalk.cyan(`  Processing ${apps.length} user applications`));
  console.log(chalk.cyan('\nChecking availability in winget (this may take a while)...\n'));

  const configApps: AppConfig[] = [];
  let checked = 0;
  let availableCount = 0;
  let unavailableCount = 0;

  for (const app of apps) {
    checked++;
    process.stdout.write(`\r  Checking ${checked}/${apps.length}: ${app.name.substring(0, 40).padEnd(40)}`);

    // Apps with 'winget' source are definitely available
    const isFromWinget = app.source?.toLowerCase() === 'winget';
    let availableInWinget = isFromWinget;

    // For non-winget sources, check if available
    if (!isFromWinget && app.id) {
      availableInWinget = await winget.isAvailableInWinget(app.id);
    }

    if (availableInWinget) {
      availableCount++;
    } else {
      unavailableCount++;
    }

    const configApp: AppConfig = {
      id: app.id,
      name: app.name,
      version: 'latest',
      availableInWinget
    };

    if (!availableInWinget) {
      configApp.note = 'Not available via winget - install manually';
    }

    configApps.push(configApp);
  }

  console.log('\n');

  // Sort: available first, then by name
  configApps.sort((a, b) => {
    if (a.availableInWinget !== b.availableInWinget) {
      return a.availableInWinget ? -1 : 1;
    }
    return a.name.localeCompare(b.name);
  });

  const config: Config = {
    apps: configApps
  };

  // Write to file
  const absolutePath = path.resolve(outputPath);
  const dir = path.dirname(absolutePath);

  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true });
  }

  fs.writeFileSync(absolutePath, JSON.stringify(config, null, 2), 'utf-8');

  console.log(chalk.green(`✓ Config exported to: ${absolutePath}`));
  console.log(chalk.gray(`\nSummary:`));
  console.log(chalk.green(`  Available in winget: ${availableCount}`));
  console.log(chalk.yellow(`  Not available: ${unavailableCount}`));
  console.log(chalk.gray(`  Excluded (system): ${excludedCount}`));
  console.log(chalk.gray(`  Total exported: ${apps.length}\n`));

  if (unavailableCount > 0) {
    console.log(chalk.yellow('Note: Applications marked as unavailable will be skipped during installation.'));
    console.log(chalk.yellow('You can manually install them or find alternatives in winget.\n'));
  }
}

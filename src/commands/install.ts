import fs from 'fs';
import path from 'path';
import chalk from 'chalk';
import Table from 'cli-table3';
import { execSync } from 'child_process';
import { Config } from '../types/config';
import { WingetService } from '../services/winget';

function createExeAliases(): string[] {
  const userHome = process.env.USERPROFILE || '';
  const wingetLinksPath = path.join(userHome, 'AppData', 'Local', 'Microsoft', 'WinGet', 'Links');
  const wingetPackagesPath = path.join(userHome, 'AppData', 'Local', 'Microsoft', 'WinGet', 'Packages');

  const aliasesCreated: string[] = [];

  // Create Links directory if it doesn't exist
  if (!fs.existsSync(wingetLinksPath)) {
    try {
      fs.mkdirSync(wingetLinksPath, { recursive: true });
    } catch {
      return aliasesCreated;
    }
  }

  if (!fs.existsSync(wingetPackagesPath)) {
    return aliasesCreated;
  }

  try {
    const packages = fs.readdirSync(wingetPackagesPath);
    for (const pkg of packages) {
      const pkgPath = path.join(wingetPackagesPath, pkg);
      if (!fs.statSync(pkgPath).isDirectory()) continue;

      const files = fs.readdirSync(pkgPath);
      for (const file of files) {
        if (!file.endsWith('.exe')) continue;

        // Extract simple name from complex exe names like "codex-x86_64-pc-windows-msvc.exe"
        let simpleName = file;

        // Remove architecture suffixes
        simpleName = simpleName.replace(/-x86_64-pc-windows-msvc\.exe$/i, '.exe');
        simpleName = simpleName.replace(/-x64\.exe$/i, '.exe');
        simpleName = simpleName.replace(/-x86\.exe$/i, '.exe');
        simpleName = simpleName.replace(/-win64\.exe$/i, '.exe');
        simpleName = simpleName.replace(/-win32\.exe$/i, '.exe');
        simpleName = simpleName.replace(/-windows\.exe$/i, '.exe');

        // If name was simplified, create an alias
        if (simpleName !== file) {
          const aliasPath = path.join(wingetLinksPath, simpleName);
          const targetPath = path.join(pkgPath, file);

          if (!fs.existsSync(aliasPath)) {
            try {
              // Copy the exe as alias (symlinks require admin on Windows)
              fs.copyFileSync(targetPath, aliasPath);
              aliasesCreated.push(simpleName.replace('.exe', ''));
            } catch {
              // Try creating a cmd wrapper instead
              const cmdPath = aliasPath.replace('.exe', '.cmd');
              if (!fs.existsSync(cmdPath)) {
                try {
                  fs.writeFileSync(cmdPath, `@echo off\n"${targetPath}" %*`);
                  aliasesCreated.push(simpleName.replace('.exe', ''));
                } catch {
                  // Ignore
                }
              }
            }
          }
        }
      }
    }
  } catch {
    // Ignore errors
  }

  return aliasesCreated;
}

function ensureWingetPathsConfigured(): { linksAdded: boolean; packagesAdded: string[] } {
  const userHome = process.env.USERPROFILE || '';
  const wingetLinksPath = path.join(userHome, 'AppData', 'Local', 'Microsoft', 'WinGet', 'Links');
  const wingetPackagesPath = path.join(userHome, 'AppData', 'Local', 'Microsoft', 'WinGet', 'Packages');

  let linksAdded = false;
  const packagesAdded: string[] = [];

  try {
    // Get current user PATH
    const currentPath = execSync('powershell -Command "[Environment]::GetEnvironmentVariable(\'Path\', \'User\')"', {
      encoding: 'utf-8'
    }).trim();

    const pathsToAdd: string[] = [];

    // Check WinGet Links
    if (fs.existsSync(wingetLinksPath)) {
      if (!currentPath.toLowerCase().includes(wingetLinksPath.toLowerCase())) {
        pathsToAdd.push(wingetLinksPath);
        linksAdded = true;
      }
    }

    // Check WinGet Packages - find directories with .exe files
    if (fs.existsSync(wingetPackagesPath)) {
      const packages = fs.readdirSync(wingetPackagesPath);
      for (const pkg of packages) {
        const pkgPath = path.join(wingetPackagesPath, pkg);
        if (fs.statSync(pkgPath).isDirectory()) {
          // Check if this package dir has exe files and is not already in PATH
          const files = fs.readdirSync(pkgPath);
          const hasExe = files.some(f => f.endsWith('.exe'));
          if (hasExe && !currentPath.toLowerCase().includes(pkgPath.toLowerCase())) {
            pathsToAdd.push(pkgPath);
            packagesAdded.push(pkg.split('_')[0]); // Get package name before underscore
          }
        }
      }
    }

    // Add all paths at once
    if (pathsToAdd.length > 0) {
      const newPath = currentPath ? `${currentPath};${pathsToAdd.join(';')}` : pathsToAdd.join(';');
      execSync(`powershell -Command "[Environment]::SetEnvironmentVariable('Path', '${newPath.replace(/'/g, "''")}', 'User')"`, {
        encoding: 'utf-8'
      });

      // Also update current process PATH
      process.env.PATH = `${process.env.PATH};${pathsToAdd.join(';')}`;
    }

    return { linksAdded, packagesAdded };
  } catch {
    return { linksAdded: false, packagesAdded: [] };
  }
}

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

  // Ensure WinGet paths are configured
  if (successful > 0) {
    // Create aliases for exe files with complex names
    const aliases = createExeAliases();
    if (aliases.length > 0) {
      console.log(chalk.cyan('📌 Created command aliases:'));
      for (const alias of aliases) {
        console.log(chalk.gray(`   - ${alias}`));
      }
    }

    const { linksAdded, packagesAdded } = ensureWingetPathsConfigured();
    if (linksAdded || packagesAdded.length > 0) {
      console.log(chalk.cyan('📌 PATH updated for installed applications:'));
      if (linksAdded) {
        console.log(chalk.gray('   - WinGet Links'));
      }
      for (const pkg of packagesAdded) {
        console.log(chalk.gray(`   - ${pkg}`));
      }
    }

    if (aliases.length > 0 || linksAdded || packagesAdded.length > 0) {
      console.log(chalk.yellow('\n⚠️  Restart your terminal to use installed commands.\n'));
    }
  }
}

import Table from 'cli-table3';
import chalk from 'chalk';
import * as fs from 'fs';
import * as path from 'path';
import * as readline from 'readline';
import { WingetService } from '../services/winget';
import { AppConfig, Config, InstalledApp } from '../types/config';
import { PRESET_DIR } from '../constants';

const PAGE_SIZE = 10;

export async function createCommand(fileName: string): Promise<void> {
  const winget = new WingetService();
  const selectedApps: AppConfig[] = [];
  let allApps: InstalledApp[] = [];
  let filteredApps: InstalledApp[] = [];
  let installedMap: Map<string, string> = new Map();
  let cursorIndex = 0;
  let filterMode = false;
  let filterText = '';

  const configPath = fileName.endsWith('.json') ? fileName : `${fileName}.json`;
  const fullPath = path.join(PRESET_DIR, configPath);

  // Ensure PRESET_DIR exists
  if (!fs.existsSync(PRESET_DIR)) {
    fs.mkdirSync(PRESET_DIR, { recursive: true });
  }

  console.log(chalk.cyan(`\n🛠️  Creating config: ${chalk.bold(fullPath)}`));

  process.stdout.write(chalk.gray('Loading installed applications... '));
  const installedApps = winget.getInstalledApps();
  const currentInstalledMap = new Map<string, string>();
  const currentInstalledByName = new Map<string, string>();
  
  installedApps.forEach(app => {
    if (app.id) currentInstalledMap.set(app.id, app.version);
    if (app.name) currentInstalledByName.set(app.name.toLowerCase(), app.version);
  });
  
  installedMap = currentInstalledMap;
  const installedByName = currentInstalledByName;
  console.log(chalk.green(`Done. (${installedMap.size} IDs, ${installedByName.size} names)`));

  process.stdout.write(chalk.gray('Loading all applications from winget... '));
  allApps = await winget.searchApp('');
  filteredApps = [...allApps];
  console.log(chalk.green(`Done. (${filteredApps.length} apps)\n`));

  const getCurrentPage = () => Math.floor(cursorIndex / PAGE_SIZE);
  const getTotalPages = () => Math.ceil(filteredApps.length / PAGE_SIZE);

  const getCurrentPageApps = () => {
    const page = getCurrentPage();
    const start = page * PAGE_SIZE;
    return filteredApps.slice(start, start + PAGE_SIZE);
  };

  const clearScreen = () => {
    process.stdout.write('\x1B[2J\x1B[H');
  };

  const render = () => {
    clearScreen();

    // Header
    console.log(chalk.cyan.bold(`🛠️  Creating config: ${configPath}`));
    console.log();

    // Selected apps
    if (selectedApps.length > 0) {
      console.log(chalk.green.bold(`⭐ Selected (${selectedApps.length}):`));
      const maxShow = 5;
      selectedApps.slice(0, maxShow).forEach(app => {
        console.log(chalk.green(`   ${app.name}`));
      });
      if (selectedApps.length > maxShow) {
        console.log(chalk.gray(`   ... and ${selectedApps.length - maxShow} more`));
      }
      console.log();
    }

    // Filter status
    if (filterMode) {
      console.log(chalk.yellow.bold(`🔍 Filter: ${filterText}_`));
      console.log();
    } else if (filterText) {
      console.log(chalk.blue(`🔍 Filtered by: "${filterText}" (${filteredApps.length} results)`));
      console.log();
    }

    // Apps table
    const totalPages = getTotalPages();
    const currentPage = getCurrentPage();
    console.log(chalk.bold(`Applications (Page ${currentPage + 1}/${totalPages || 1}):`));

    if (filteredApps.length === 0) {
      console.log(chalk.yellow('  No applications found.'));
    } else {
      const pageApps = getCurrentPageApps();
      const pageStart = currentPage * PAGE_SIZE;

      const table = new Table({
        head: [' ', 'Name', 'ID', 'Version'],
        colWidths: [5, 33, 40, 14],
        style: { head: ['cyan'], border: ['gray'] }
      });

      pageApps.forEach((app, i) => {
        const globalIdx = pageStart + i;
        const isSelected = selectedApps.some(s => s.id === app.id);
        
        let installedVersion = installedMap.get(app.id);
        if (installedVersion === undefined) {
          installedVersion = installedByName.get(app.name.toLowerCase());
        }
        
        const isInstalled = installedVersion !== undefined;
        const isCursor = globalIdx === cursorIndex;

        // Status icon: ✓ installed, ● selected, ○ empty
        let statusIcon: string;
        if (isInstalled) {
          statusIcon = chalk.green('✓');
        } else if (isSelected) {
          statusIcon = chalk.blue('●');
        } else {
          statusIcon = chalk.gray('○');
        }

        // Highlight entire row if cursor is on it
        const name = app.name.substring(0, 32);
        const id = app.id.substring(0, 38);
        
        let versionText = app.version.substring(0, 12);
        if (isInstalled && installedVersion !== app.version) {
          versionText = chalk.yellow(versionText);
        } else if (!isCursor) {
          versionText = chalk.gray(versionText);
        }

        if (isCursor) {
          const row = [
            chalk.bgCyan.black(` ${statusIcon} `),
            chalk.bgCyan.black(name.padEnd(32)),
            chalk.bgCyan.black(id.padEnd(38)),
            chalk.bgCyan.black(app.version.substring(0, 12).padEnd(12)) // Reset version color for cursor
          ];
          // Re-apply yellow if needed even on cursor for visibility
          if (isInstalled && installedVersion !== app.version) {
             row[3] = chalk.bgCyan.yellow(app.version.substring(0, 12).padEnd(12));
          }
          table.push(row);
        } else {
          table.push([` ${statusIcon} `, name, chalk.gray(id), versionText]);
        }
      });

      console.log(table.toString());
    }

    // Legend & Help
    console.log();
    console.log(chalk.gray('─'.repeat(60)));
    console.log(`${chalk.green('✓')} installed  ${chalk.blue('●')} selected  ${chalk.gray('○')} not selected`);
    console.log();
    console.log(chalk.bold('Controls:'));
    console.log(`  ${chalk.yellow('↑/↓')}     Navigate       ${chalk.yellow('Enter')}   Toggle selection`);
    console.log(`  ${chalk.yellow('f')}       Filter          ${chalk.yellow('Esc')}     Clear filter`);
    console.log(`  ${chalk.yellow('s')}       Save & exit     ${chalk.yellow('q')}       Quit without saving`);
  };

  const applyFilter = () => {
    if (filterText) {
      const q = filterText.toLowerCase();
      filteredApps = allApps.filter(app =>
        app.name.toLowerCase().includes(q) ||
        app.id.toLowerCase().includes(q)
      );
    } else {
      filteredApps = [...allApps];
    }
    cursorIndex = 0;
  };

  const toggleSelection = () => {
    if (filteredApps.length === 0) return;

    const app = filteredApps[cursorIndex];
    const index = selectedApps.findIndex(s => s.id === app.id);

    if (index === -1) {
      selectedApps.push({
        id: app.id,
        name: app.name,
        version: app.version,
        availableInWinget: true
      });
    } else {
      selectedApps.splice(index, 1);
    }
  };

  const save = () => {
    if (selectedApps.length === 0) {
      return false;
    }
    const config: Config = { apps: selectedApps };
    fs.writeFileSync(fullPath, JSON.stringify(config, null, 2));
    return true;
  };

  // Setup raw mode for keyboard input
  readline.emitKeypressEvents(process.stdin);
  if (process.stdin.isTTY) {
    process.stdin.setRawMode(true);
  }

  render();

  return new Promise((resolve) => {
    const cleanup = () => {
      if (process.stdin.isTTY) {
        process.stdin.setRawMode(false);
      }
      process.stdin.removeAllListeners('keypress');
      clearScreen();
    };

    process.stdin.on('keypress', (_str, key) => {
      if (!key) return;

      // Handle Ctrl+C
      if (key.ctrl && key.name === 'c') {
        cleanup();
        console.log(chalk.yellow('Cancelled.'));
        process.exit(0);
      }

      if (filterMode) {
        // Filter input mode
        if (key.name === 'return' || key.name === 'escape') {
          filterMode = false;
          applyFilter();
          render();
        } else if (key.name === 'backspace') {
          filterText = filterText.slice(0, -1);
          render();
        } else if (key.sequence && key.sequence.length === 1 && key.sequence.charCodeAt(0) >= 32) {
          filterText += key.sequence;
          render();
        }
        return;
      }

      // Normal mode
      switch (key.name) {
        case 'up':
        case 'k':
          if (cursorIndex > 0) {
            cursorIndex--;
            render();
          }
          break;

        case 'down':
        case 'j':
          if (cursorIndex < filteredApps.length - 1) {
            cursorIndex++;
            render();
          }
          break;

        case 'pageup':
          cursorIndex = Math.max(0, cursorIndex - PAGE_SIZE);
          render();
          break;

        case 'pagedown':
          cursorIndex = Math.min(filteredApps.length - 1, cursorIndex + PAGE_SIZE);
          render();
          break;

        case 'return':
        case 'space':
          toggleSelection();
          render();
          break;

        case 'f':
          filterMode = true;
          filterText = '';
          render();
          break;

        case 'escape':
          filterText = '';
          applyFilter();
          render();
          break;

        case 's':
          if (save()) {
            cleanup();
            console.log(chalk.green(`✅ Saved ${selectedApps.length} apps to ${fullPath}`));
            resolve();
          } else {
            // Flash message - no apps selected
            clearScreen();
            console.log(chalk.red.bold('\n  ⚠️  No applications selected!\n'));
            setTimeout(() => render(), 1000);
          }
          break;

        case 'q':
          cleanup();
          console.log(chalk.yellow('Exited without saving.'));
          resolve();
          break;
      }
    });
  });
}

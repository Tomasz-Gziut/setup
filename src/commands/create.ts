import Table from 'cli-table3';
import chalk from 'chalk';
import * as fs from 'fs';
import * as path from 'path';
import * as readline from 'readline';
import { WingetService } from '../services/winget';
import { AppConfig, Config, InstalledApp } from '../types/config';

const PAGE_SIZE = 10;

export async function createCommand(fileName: string): Promise<void> {
  const winget = new WingetService();
  const selectedApps: AppConfig[] = [];
  let allApps: InstalledApp[] = [];
  let filteredApps: InstalledApp[] = [];
  let cursorIndex = 0;
  let filterMode = false;
  let filterText = '';

  const configPath = fileName.endsWith('.json') ? fileName : `${fileName}.json`;
  const fullPath = path.resolve(process.cwd(), configPath);

  console.log(chalk.cyan(`\n🛠️  Creating config: ${chalk.bold(configPath)}`));
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
        head: ['', 'Name', 'ID', 'Version'],
        colWidths: [4, 35, 40, 15],
        style: { head: ['cyan'], border: ['gray'] }
      });

      pageApps.forEach((app, i) => {
        const globalIdx = pageStart + i;
        const isSelected = selectedApps.some(s => s.id === app.id);
        const isCursor = globalIdx === cursorIndex;

        const prefix = isCursor ? chalk.cyan('▶') : ' ';
        const checkbox = isSelected ? chalk.green('[✓]') : '[ ]';
        const name = isCursor ? chalk.cyan.bold(app.name.substring(0, 33)) : app.name.substring(0, 33);
        const id = isCursor ? chalk.cyan(app.id.substring(0, 38)) : chalk.gray(app.id.substring(0, 38));
        const version = chalk.gray(app.version.substring(0, 13));

        table.push([`${prefix}${checkbox}`, name, id, version]);
      });

      console.log(table.toString());
    }

    // Help
    console.log();
    console.log(chalk.gray('─'.repeat(60)));
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

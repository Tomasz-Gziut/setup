import Table from 'cli-table3';
import chalk from 'chalk';
import * as fs from 'fs';
import * as path from 'path';
import * as readline from 'readline';
import { WingetService } from '../services/winget';
import { AppConfig, Config, InstalledApp } from '../types/config';
import { PRESET_DIR } from '../constants';

const PAGE_SIZE = 10;

type InstalledMatch = {
  version: string;
  source: string;
};

export async function createCommand(fileName: string): Promise<void> {
  const winget = new WingetService();
  const selectedApps: AppConfig[] = [];
  let allApps: InstalledApp[] = [];
  let filteredApps: InstalledApp[] = [];
  let installedMap: Map<string, InstalledMatch> = new Map();
  let installedNameEntries: Array<{ normalizedName: string; match: InstalledMatch }> = [];
  const searchCache: Map<string, InstalledApp[]> = new Map();
  let filterSearchTimer: NodeJS.Timeout | undefined;
  let filterSearchToken = 0;
  let filterSearching = false;
  let cursorIndex = 0;
  let filterText = '';

  const configPath = fileName.endsWith('.json') ? fileName : `${fileName}.json`;
  const fullPath = path.join(PRESET_DIR, configPath);

  // Ensure PRESET_DIR exists
  if (!fs.existsSync(PRESET_DIR)) {
    fs.mkdirSync(PRESET_DIR, { recursive: true });
  }

  console.log(chalk.cyan(`\n🛠️  Creating config: ${chalk.bold(fullPath)}`));

  const normalizeAppName = (name: string): string =>
    name
      .toLowerCase()
      .replace(/\b(x64|x86|win64|win32|stable|browser)\b/g, ' ')
      .replace(/[^a-z0-9]+/g, ' ')
      .replace(/\s+/g, ' ')
      .trim();

  process.stdout.write(chalk.gray('Loading installed applications... '));
  const installedApps = winget.getInstalledApps();
  const currentInstalledMap = new Map<string, InstalledMatch>();
  const currentInstalledByName = new Map<string, InstalledMatch>();
  const currentInstalledNameEntries: Array<{ normalizedName: string; match: InstalledMatch }> = [];
  
  installedApps.forEach(app => {
    const installedMatch = {
      version: app.version,
      source: app.source || 'local/other'
    };

    if (app.id) currentInstalledMap.set(app.id.toLowerCase(), installedMatch);
    if (app.name) {
      const normalizedName = normalizeAppName(app.name);
      if (normalizedName) {
        currentInstalledByName.set(normalizedName, installedMatch);
        currentInstalledNameEntries.push({ normalizedName, match: installedMatch });
      }
    }
  });
  
  installedMap = currentInstalledMap;
  installedNameEntries = currentInstalledNameEntries;
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

  const mergeApps = (...appLists: InstalledApp[][]): InstalledApp[] => {
    const appsById = new Map<string, InstalledApp>();

    appLists.flat().forEach(app => {
      if (!app.id) return;
      const key = app.id.toLowerCase();
      if (!appsById.has(key)) {
        appsById.set(key, app);
      }
    });

    return Array.from(appsById.values());
  };

  const findInstalledMatch = (app: InstalledApp): InstalledMatch | undefined => {
    const installedById = installedMap.get(app.id.toLowerCase());
    if (installedById !== undefined) return installedById;

    const normalizedName = normalizeAppName(app.name);
    if (!normalizedName) return undefined;

    const exactNameMatch = installedByName.get(normalizedName);
    if (exactNameMatch !== undefined) return exactNameMatch;

    const fuzzyNameMatch = installedNameEntries.find(installed =>
      installed.normalizedName.length >= 3 &&
      normalizedName.length >= 3 &&
      (installed.normalizedName.includes(normalizedName) ||
        normalizedName.includes(installed.normalizedName))
    );

    return fuzzyNameMatch?.match;
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
    const filterLabel = filterText ? `Filter: ${filterText}_ (${filteredApps.length} results)` : 'Filter: _';
    console.log(chalk.yellow.bold(filterLabel));
    console.log();

    // Apps table
    const totalPages = getTotalPages();
    const currentPage = getCurrentPage();
    console.log(chalk.bold(`Applications (Page ${currentPage + 1}/${totalPages || 1}):`));
    if (filterSearching) {
      console.log(chalk.gray('  Searching winget source...'));
    }

    if (filteredApps.length === 0) {
      console.log(chalk.yellow('  No applications found.'));
    } else {
      const pageApps = getCurrentPageApps();
      const pageStart = currentPage * PAGE_SIZE;

      const table = new Table({
        head: [' ', 'Name', 'ID', 'Version', 'Source'],
        colWidths: [5, 30, 35, 13, 15],
        style: { head: ['cyan'], border: ['gray'] }
      });

      pageApps.forEach((app, i) => {
        const globalIdx = pageStart + i;
        const isSelected = selectedApps.some(s => s.id === app.id);
        
        const installedMatch = findInstalledMatch(app);
        const installedVersion = installedMatch?.version;
        const source = installedMatch?.source ?? app.source;
        
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
        const name = app.name.substring(0, 29);
        const id = app.id.substring(0, 33);
        const sourceText = (source || 'N/A').substring(0, 13);
        
        let versionText = app.version.substring(0, 12);
        if (isInstalled && installedVersion !== app.version) {
          versionText = chalk.yellow(versionText);
        } else if (!isCursor) {
          versionText = chalk.gray(versionText);
        }

        if (isCursor) {
          const row = [
            chalk.bgCyan.black(` ${statusIcon} `),
            chalk.bgCyan.black(name.padEnd(29)),
            chalk.bgCyan.black(id.padEnd(33)),
            chalk.bgCyan.black(app.version.substring(0, 12).padEnd(12)),
            chalk.bgCyan.black(sourceText.padEnd(13))
          ];
          // Re-apply yellow if needed even on cursor for visibility
          if (isInstalled && installedVersion !== app.version) {
             row[3] = chalk.bgCyan.yellow(app.version.substring(0, 12).padEnd(12));
          }
          table.push(row);
        } else {
          table.push([` ${statusIcon} `, name, chalk.gray(id), versionText, chalk.gray(sourceText)]);
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
    console.log(`  ${chalk.yellow('Type')}    Filter         ${chalk.yellow('Backspace')} Delete char`);
    console.log(`  ${chalk.yellow('?/?')}     Navigate       ${chalk.yellow('Enter')}     Toggle selection`);
    console.log(`  ${chalk.yellow('Ctrl+S')}  Save & exit    ${chalk.yellow('Ctrl+Q')}    Quit without saving`);
  };

  const getLocalMatches = (q: string): InstalledApp[] =>
    allApps.filter(app =>
      app.name.toLowerCase().includes(q) ||
      app.id.toLowerCase().includes(q)
    );

  const applyLocalFilter = () => {
    const trimmedFilter = filterText.trim();

    if (trimmedFilter) {
      filteredApps = getLocalMatches(trimmedFilter.toLowerCase());
    } else {
      filteredApps = [...allApps];
    }
    cursorIndex = 0;
  };

  const applyWingetFilter = async (trimmedFilter: string) => {
    const q = trimmedFilter.toLowerCase();
    let wingetMatches = searchCache.get(q);

    if (wingetMatches === undefined) {
      filterSearching = true;
      render();
      wingetMatches = await winget.searchApp(trimmedFilter);
      searchCache.set(q, wingetMatches);
      filterSearching = false;
    }

    allApps = mergeApps(allApps, wingetMatches);

    if (filterText.trim().toLowerCase() === q) {
      filteredApps = mergeApps(getLocalMatches(q), wingetMatches);
      cursorIndex = 0;
    }
  };

  const scheduleWingetFilter = () => {
    if (filterSearchTimer) {
      clearTimeout(filterSearchTimer);
    }

    const trimmedFilter = filterText.trim();
    if (trimmedFilter.length < 3) {
      filterSearching = false;
      return;
    }

    const token = ++filterSearchToken;
    filterSearchTimer = setTimeout(async () => {
      if (token !== filterSearchToken) return;
      await applyWingetFilter(trimmedFilter);
      if (token === filterSearchToken) render();
    }, 450);
  };

  const applyFilter = async () => {
    if (filterSearchTimer) {
      clearTimeout(filterSearchTimer);
    }

    applyLocalFilter();

    const trimmedFilter = filterText.trim();
    if (trimmedFilter.length >= 3) {
      await applyWingetFilter(trimmedFilter);
    }
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
      if (filterSearchTimer) {
        clearTimeout(filterSearchTimer);
      }
      if (process.stdin.isTTY) {
        process.stdin.setRawMode(false);
      }
      process.stdin.removeAllListeners('keypress');
      clearScreen();
    };

    const saveAndExit = () => {
      if (save()) {
        cleanup();
        console.log(chalk.green(`? Saved ${selectedApps.length} apps to ${fullPath}`));
        resolve();
      } else {
        clearScreen();
        console.log(chalk.red.bold('\n  ??  No applications selected!\n'));
        setTimeout(() => render(), 1000);
      }
    };

    process.stdin.on('keypress', async (_str, key) => {
      if (!key) return;

      // Handle Ctrl+C
      if (key.ctrl && key.name === 'c') {
        cleanup();
        console.log(chalk.yellow('Cancelled.'));
        process.exit(0);
      }

      if (key.ctrl && key.name === 's') {
        saveAndExit();
        return;
      }

      if (key.ctrl && key.name === 'q') {
        cleanup();
        console.log(chalk.yellow('Exited without saving.'));
        resolve();
        return;
      }

      if (key.name === 'backspace') {
        filterText = filterText.slice(0, -1);
        applyLocalFilter();
        scheduleWingetFilter();
        render();
        return;
      }

      if (!key.ctrl && !key.meta && key.sequence && key.sequence.length === 1 && key.sequence.charCodeAt(0) >= 32) {
        filterText += key.sequence;
        applyLocalFilter();
        scheduleWingetFilter();
        render();
        return;
      }

      // Navigation mode
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

        case 'escape':
          render();
          break;
      }
    });
  });
}

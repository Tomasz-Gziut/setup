import Table from 'cli-table3';
import chalk from 'chalk';
import { WingetService } from '../services/winget';

export async function showCommand(): Promise<void> {
  const winget = new WingetService();

  console.log(chalk.cyan('\n📦 Fetching installed applications...\n'));

  try {
    const apps = winget.getInstalledApps();

    if (apps.length === 0) {
      console.log(chalk.yellow('No applications found.'));
      return;
    }

    const table = new Table({
      head: [
        chalk.bold.white('Name'),
        chalk.bold.white('ID'),
        chalk.bold.white('Version'),
        chalk.bold.white('Source')
      ],
      style: {
        head: [],
        border: ['gray']
      },
      colWidths: [40, 45, 20, 15]
    });

    for (const app of apps) {
      table.push([
        app.name.substring(0, 38),
        chalk.cyan(app.id.substring(0, 43)),
        chalk.green(app.version.substring(0, 18)),
        app.source ? chalk.magenta(app.source) : chalk.gray('N/A')
      ]);
    }

    console.log(table.toString());
    console.log(chalk.gray(`\nTotal: ${apps.length} applications\n`));
  } catch (error: any) {
    console.error(chalk.red('Error fetching applications:'), error.message);
    console.log(chalk.yellow('\nMake sure winget is installed and available in PATH.'));
    process.exit(1);
  }
}

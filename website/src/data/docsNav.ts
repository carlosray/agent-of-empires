export interface NavItem {
  title: string;
  href: string;
}

export interface NavSection {
  title: string;
  items: NavItem[];
}

export const docsNav: NavSection[] = [
  {
    title: "Getting Started",
    items: [
      { title: "Introduction", href: "/docs/" },
      { title: "Installation", href: "/docs/installation/" },
      { title: "Quick Start", href: "/docs/quick-start/" },
    ],
  },
  {
    title: "Guides",
    items: [
      { title: "Docker Sandbox", href: "/guides/sandbox/" },
      { title: "Web Dashboard", href: "/guides/web-dashboard/" },
      { title: "Remote Phone Access", href: "/guides/remote-phone-access/" },
      { title: "Repo Config & Hooks", href: "/guides/repo-config/" },
      { title: "Git Worktrees", href: "/guides/worktrees/" },
      { title: "Diff View", href: "/guides/diff-view/" },
      { title: "tmux Status Bar", href: "/guides/tmux-status-bar/" },
      { title: "Sound Effects", href: "/docs/sounds/" },
    ],
  },
  {
    title: "Fork Features",
    items: [
      { title: "Overview", href: "/docs/fork-features/" },
      { title: "Git Branch Labels", href: "/docs/fork-features/git-branch-display/" },
      { title: "Tool Session Tracking", href: "/docs/fork-features/tool-session-tracking/" },
      { title: "Terminal Tab Titles", href: "/docs/fork-features/terminal-tab-title/" },
    ],
  },
  {
    title: "Reference",
    items: [
      { title: "CLI Reference", href: "/docs/cli/reference/" },
      { title: "Configuration", href: "/docs/guides/configuration/" },
    ],
  },
  {
    title: "Contributing",
    items: [
      { title: "Development", href: "/docs/development/" },
    ],
  },
];

export function getFlatNavItems(): NavItem[] {
  return docsNav.flatMap((section) => section.items);
}

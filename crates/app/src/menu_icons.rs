//! One map from a menu action to its icon (B16), so the same action has the same icon in every
//! menu and on every bar: Copy is always [`COPY`], Delete always [`DELETE`]. Menus name these
//! instead of a `Lucide::…` of their own; a new action gets its constant here.
//!
//! The menu look itself (28 px rows, 14 px icons in a 16 px slot, section headers, the danger
//! colour) lives in the shared component, `vendor/gpui-component/src/menu/popup_menu.rs`.
//! `crate::menu_tests` fails on a menu item with a label and no icon.

#![allow(dead_code)] // A shared vocabulary: not every action is in a menu yet.

use gpui_kit::assets::IconName as Lucide;

// Clipboard and editing.
pub const COPY: Lucide = Lucide::Copy;
pub const CUT: Lucide = Lucide::Scissors;
pub const PASTE: Lucide = Lucide::ClipboardPaste;
pub const DUPLICATE: Lucide = Lucide::CopyPlus;
pub const RENAME: Lucide = Lucide::Pencil;
pub const EDIT: Lucide = Lucide::SquarePen;
pub const DELETE: Lucide = Lucide::Trash;
pub const SELECT_ALL: Lucide = Lucide::SquareDashed;
pub const UNDO: Lucide = Lucide::Undo2;
pub const REDO: Lucide = Lucide::Redo2;
pub const LINK: Lucide = Lucide::Link;

// Files and places.
pub const NEW_FILE: Lucide = Lucide::FilePlus;
pub const NEW_FOLDER: Lucide = Lucide::FolderPlus;
pub const OPEN: Lucide = Lucide::FolderOpen;
pub const OPEN_FILE: Lucide = Lucide::File;
pub const REVEAL: Lucide = Lucide::FolderOpen;
pub const OPEN_TERMINAL: Lucide = Lucide::SquareTerminal;
pub const OPEN_BROWSER: Lucide = Lucide::Globe;
pub const OPEN_EXTERNAL: Lucide = Lucide::ExternalLink;
pub const OPEN_CANVAS: Lucide = Lucide::Shapes;
pub const SAVE: Lucide = Lucide::Save;
pub const EXPORT: Lucide = Lucide::Download;
pub const IMPORT: Lucide = Lucide::FolderInput;
pub const UPLOAD: Lucide = Lucide::Upload;
pub const HTML: Lucide = Lucide::FileCode;
pub const REACT: Lucide = Lucide::Code;
pub const IMAGE: Lucide = Lucide::Image;
pub const VECTOR: Lucide = Lucide::PenTool;

// Running things.
pub const NEW: Lucide = Lucide::Plus;
pub const NEW_TERMINAL: Lucide = Lucide::SquareTerminal;
pub const NEW_WINDOW: Lucide = Lucide::AppWindow;
pub const RUN: Lucide = Lucide::Play;
pub const STOP: Lucide = Lucide::CircleStop;
pub const RESTART: Lucide = Lucide::RotateCcw;
pub const REFRESH: Lucide = Lucide::RefreshCw;
pub const PREVIEW: Lucide = Lucide::Eye;
pub const CONNECT: Lucide = Lucide::Plug;
pub const DISCONNECT: Lucide = Lucide::Unplug;
pub const AGENT: Lucide = Lucide::Bot;
pub const SEND: Lucide = Lucide::Send;

// Tabs and layout.
pub const CLOSE: Lucide = Lucide::X;
pub const CLOSE_OTHERS: Lucide = Lucide::CircleX;
pub const CLOSE_ALL: Lucide = Lucide::CircleX;
pub const PIN: Lucide = Lucide::Pin;
pub const UNPIN: Lucide = Lucide::PinOff;
pub const SPLIT_RIGHT: Lucide = Lucide::Columns2;
pub const SPLIT_DOWN: Lucide = Lucide::Rows2;
pub const MAXIMIZE: Lucide = Lucide::Maximize;
pub const MOVE: Lucide = Lucide::MoveRight;
pub const HIDE: Lucide = Lucide::EyeOff;
pub const SHOW: Lucide = Lucide::Eye;

// Accounts, settings and the app.
pub const SETTINGS: Lucide = Lucide::Settings;
pub const SIGN_IN: Lucide = Lucide::LogIn;
pub const SIGN_OUT: Lucide = Lucide::LogOut;
pub const ACCOUNT: Lucide = Lucide::CircleUser;
pub const ADD_ACCOUNT: Lucide = Lucide::UserPlus;
pub const KEYBOARD: Lucide = Lucide::Keyboard;
pub const INSTALL: Lucide = Lucide::Download;
pub const UPDATE: Lucide = Lucide::CircleArrowUp;
pub const UNINSTALL: Lucide = Lucide::Trash;
pub const HELP: Lucide = Lucide::Info;
pub const DOCS: Lucide = Lucide::BookOpen;
pub const FILTER: Lucide = Lucide::ListFilter;
pub const SORT: Lucide = Lucide::ArrowUpDown;
pub const PAGES: Lucide = Lucide::LayoutGrid;
pub const PERMISSIONS: Lucide = Lucide::ShieldCheck;
pub const KEY: Lucide = Lucide::KeyRound;

// Git.
pub const BRANCH: Lucide = Lucide::GitBranch;
pub const NEW_BRANCH: Lucide = Lucide::GitBranchPlus;
pub const DELETE_BRANCH: Lucide = Lucide::GitBranchMinus;
pub const MERGE: Lucide = Lucide::GitMerge;
pub const PULL_REQUEST: Lucide = Lucide::GitPullRequest;
pub const COMMIT: Lucide = Lucide::GitCommitHorizontal;
pub const HISTORY: Lucide = Lucide::GitGraph;

// Data.
pub const DATABASE: Lucide = Lucide::Database;
pub const TABLE: Lucide = Lucide::Table;
pub const QUERY: Lucide = Lucide::FileCode;
pub const SERVER: Lucide = Lucide::Server;
pub const BACKUP: Lucide = Lucide::Archive;
pub const ERASE: Lucide = Lucide::Eraser;
pub const DONE: Lucide = Lucide::Check;
pub const CHECK_CONNECTION: Lucide = Lucide::Stethoscope;
pub const SWITCH: Lucide = Lucide::ArrowLeftRight;
pub const WORK: Lucide = Lucide::LayoutTemplate;
pub const REMOTE: Lucide = Lucide::Cloud;
pub const LAYERS: Lucide = Lucide::Layers;
pub const HISTORY_CLOCK: Lucide = Lucide::Clock;

# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_monk_global_optspecs
	string join \n locale= h/help V/version
end

function __fish_monk_needs_command
	# Figure out if the current invocation already has a command.
	set -l cmd (commandline -opc)
	set -e cmd[1]
	argparse -s (__fish_monk_global_optspecs) -- $cmd 2>/dev/null
	or return
	if set -q argv[1]
		# Also print the command, so this can be used to figure out what it is.
		echo $argv[1]
		return 1
	end
	return 0
end

function __fish_monk_using_subcommand
	set -l cmd (__fish_monk_needs_command)
	test -z "$cmd"
	and return 1
	contains -- $cmd[1] $argv
end

complete -c monk -n "__fish_monk_needs_command" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_needs_command" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_needs_command" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_needs_command" -f -a "init" -d 'Run the interactive onboarding wizard'
complete -c monk -n "__fish_monk_needs_command" -f -a "tui" -d 'Open the interactive TUI'
complete -c monk -n "__fish_monk_needs_command" -f -a "lang" -d 'Set the interface language'
complete -c monk -n "__fish_monk_needs_command" -f -a "start" -d 'Start a focus session'
complete -c monk -n "__fish_monk_needs_command" -f -a "stop" -d 'Stop the active session'
complete -c monk -n "__fish_monk_needs_command" -f -a "panic" -d 'Request an escape from hard mode'
complete -c monk -n "__fish_monk_needs_command" -f -a "status" -d 'Show daemon and session status'
complete -c monk -n "__fish_monk_needs_command" -f -a "profiles" -d 'List profiles'
complete -c monk -n "__fish_monk_needs_command" -f -a "profile" -d 'Manage profiles'
complete -c monk -n "__fish_monk_needs_command" -f -a "apps" -d 'Manage installed applications cache'
complete -c monk -n "__fish_monk_needs_command" -f -a "stats" -d 'Show session statistics'
complete -c monk -n "__fish_monk_needs_command" -f -a "doctor" -d 'Check environment, permissions, and daemon health'
complete -c monk -n "__fish_monk_needs_command" -f -a "config" -d 'Manage configuration'
complete -c monk -n "__fish_monk_needs_command" -f -a "daemon" -d 'Manage the background daemon'
complete -c monk -n "__fish_monk_needs_command" -f -a "menubar" -d 'Run the menu bar companion app (macOS only)'
complete -c monk -n "__fish_monk_needs_command" -f -a "completions" -d 'Generate shell completions'
complete -c monk -n "__fish_monk_needs_command" -f -a "update" -d 'Check GitHub releases for a new version and self-update'
complete -c monk -n "__fish_monk_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c monk -n "__fish_monk_using_subcommand init" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand init" -l preset -r
complete -c monk -n "__fish_monk_using_subcommand init" -l duration -r
complete -c monk -n "__fish_monk_using_subcommand init" -l hard -r -f -a "true\t''
false\t''"
complete -c monk -n "__fish_monk_using_subcommand init" -l autostart -r -f -a "true\t''
false\t''"
complete -c monk -n "__fish_monk_using_subcommand init" -l non-interactive
complete -c monk -n "__fish_monk_using_subcommand init" -s y -l yes
complete -c monk -n "__fish_monk_using_subcommand init" -l reset
complete -c monk -n "__fish_monk_using_subcommand init" -l quick -d 'Three-question quick path with sensible defaults'
complete -c monk -n "__fish_monk_using_subcommand init" -l no-daemon -d 'Skip daemon service installation'
complete -c monk -n "__fish_monk_using_subcommand init" -l no-completions -d 'Skip shell completions installation'
complete -c monk -n "__fish_monk_using_subcommand init" -l no-doctor -d 'Skip health checks'
complete -c monk -n "__fish_monk_using_subcommand init" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c monk -n "__fish_monk_using_subcommand init" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand tui" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand tui" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand tui" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand lang" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand lang" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand start" -s d -l duration -r
complete -c monk -n "__fish_monk_using_subcommand start" -l reason -r
complete -c monk -n "__fish_monk_using_subcommand start" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand start" -l hard
complete -c monk -n "__fish_monk_using_subcommand start" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand start" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand stop" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand stop" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand stop" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand panic" -l phrase -r
complete -c monk -n "__fish_monk_using_subcommand panic" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand panic" -l cancel
complete -c monk -n "__fish_monk_using_subcommand panic" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand panic" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand status" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand status" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand status" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand profiles" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand profiles" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand profiles" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand profile; and not __fish_seen_subcommand_from show edit create duplicate delete limits help" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand profile; and not __fish_seen_subcommand_from show edit create duplicate delete limits help" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand profile; and not __fish_seen_subcommand_from show edit create duplicate delete limits help" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand profile; and not __fish_seen_subcommand_from show edit create duplicate delete limits help" -f -a "show" -d 'Show a profile\'s full configuration'
complete -c monk -n "__fish_monk_using_subcommand profile; and not __fish_seen_subcommand_from show edit create duplicate delete limits help" -f -a "edit" -d 'Edit a profile (--add/--remove app id, or no flags for TTY editor)'
complete -c monk -n "__fish_monk_using_subcommand profile; and not __fish_seen_subcommand_from show edit create duplicate delete limits help" -f -a "create" -d 'Create an empty profile or copy from a preset (`--preset deepwork`)'
complete -c monk -n "__fish_monk_using_subcommand profile; and not __fish_seen_subcommand_from show edit create duplicate delete limits help" -f -a "duplicate" -d 'Duplicate an existing profile under a new name'
complete -c monk -n "__fish_monk_using_subcommand profile; and not __fish_seen_subcommand_from show edit create duplicate delete limits help" -f -a "delete" -d 'Delete a profile'
complete -c monk -n "__fish_monk_using_subcommand profile; and not __fish_seen_subcommand_from show edit create duplicate delete limits help" -f -a "limits" -d 'Set time limits for a profile (omit value to clear)'
complete -c monk -n "__fish_monk_using_subcommand profile; and not __fish_seen_subcommand_from show edit create duplicate delete limits help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from show" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from show" -l json -d 'Output machine-readable JSON'
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from show" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from show" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from edit" -l add -r
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from edit" -l remove -r
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from edit" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from edit" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from edit" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from create" -l preset -d 'Seed from a built-in preset (deepwork, study, detox, sleep, sober, lockdown, no-social, no-video, no-news, no-games, no-chat, no-shopping, no-adult, no-gambling, no-dating, no-ai)' -r
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from create" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from create" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from create" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from duplicate" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from duplicate" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from duplicate" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from delete" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from delete" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from delete" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from limits" -l max -r
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from limits" -l min -r
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from limits" -l cooldown -r
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from limits" -l daily-cap -r
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from limits" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from limits" -l clear
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from limits" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from limits" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from help" -f -a "show" -d 'Show a profile\'s full configuration'
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from help" -f -a "edit" -d 'Edit a profile (--add/--remove app id, or no flags for TTY editor)'
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from help" -f -a "create" -d 'Create an empty profile or copy from a preset (`--preset deepwork`)'
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from help" -f -a "duplicate" -d 'Duplicate an existing profile under a new name'
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from help" -f -a "delete" -d 'Delete a profile'
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from help" -f -a "limits" -d 'Set time limits for a profile (omit value to clear)'
complete -c monk -n "__fish_monk_using_subcommand profile; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c monk -n "__fish_monk_using_subcommand apps; and not __fish_seen_subcommand_from list scan help" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand apps; and not __fish_seen_subcommand_from list scan help" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand apps; and not __fish_seen_subcommand_from list scan help" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand apps; and not __fish_seen_subcommand_from list scan help" -f -a "list" -d 'List installed applications from cache'
complete -c monk -n "__fish_monk_using_subcommand apps; and not __fish_seen_subcommand_from list scan help" -f -a "scan" -d 'Force a rescan of installed applications'
complete -c monk -n "__fish_monk_using_subcommand apps; and not __fish_seen_subcommand_from list scan help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c monk -n "__fish_monk_using_subcommand apps; and __fish_seen_subcommand_from list" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand apps; and __fish_seen_subcommand_from list" -l refresh
complete -c monk -n "__fish_monk_using_subcommand apps; and __fish_seen_subcommand_from list" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand apps; and __fish_seen_subcommand_from list" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand apps; and __fish_seen_subcommand_from scan" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand apps; and __fish_seen_subcommand_from scan" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand apps; and __fish_seen_subcommand_from scan" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand apps; and __fish_seen_subcommand_from help" -f -a "list" -d 'List installed applications from cache'
complete -c monk -n "__fish_monk_using_subcommand apps; and __fish_seen_subcommand_from help" -f -a "scan" -d 'Force a rescan of installed applications'
complete -c monk -n "__fish_monk_using_subcommand apps; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c monk -n "__fish_monk_using_subcommand stats" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand stats" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand stats" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand doctor" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand doctor" -l json -d 'Output a machine-readable JSON report (exits 0 even on failures)'
complete -c monk -n "__fish_monk_using_subcommand doctor" -l fix -d 'Try to auto-fix common issues (reinstall service, install completions, etc.)'
complete -c monk -n "__fish_monk_using_subcommand doctor" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand doctor" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand config; and not __fish_seen_subcommand_from path export import help" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand config; and not __fish_seen_subcommand_from path export import help" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand config; and not __fish_seen_subcommand_from path export import help" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand config; and not __fish_seen_subcommand_from path export import help" -f -a "path"
complete -c monk -n "__fish_monk_using_subcommand config; and not __fish_seen_subcommand_from path export import help" -f -a "export"
complete -c monk -n "__fish_monk_using_subcommand config; and not __fish_seen_subcommand_from path export import help" -f -a "import"
complete -c monk -n "__fish_monk_using_subcommand config; and not __fish_seen_subcommand_from path export import help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c monk -n "__fish_monk_using_subcommand config; and __fish_seen_subcommand_from path" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand config; and __fish_seen_subcommand_from path" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand config; and __fish_seen_subcommand_from path" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand config; and __fish_seen_subcommand_from export" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand config; and __fish_seen_subcommand_from export" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand config; and __fish_seen_subcommand_from export" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand config; and __fish_seen_subcommand_from import" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand config; and __fish_seen_subcommand_from import" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand config; and __fish_seen_subcommand_from import" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand config; and __fish_seen_subcommand_from help" -f -a "path"
complete -c monk -n "__fish_monk_using_subcommand config; and __fish_seen_subcommand_from help" -f -a "export"
complete -c monk -n "__fish_monk_using_subcommand config; and __fish_seen_subcommand_from help" -f -a "import"
complete -c monk -n "__fish_monk_using_subcommand config; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c monk -n "__fish_monk_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status run install uninstall help" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status run install uninstall help" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status run install uninstall help" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status run install uninstall help" -f -a "start" -d 'Start the background daemon (user mode)'
complete -c monk -n "__fish_monk_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status run install uninstall help" -f -a "stop" -d 'Stop the running daemon'
complete -c monk -n "__fish_monk_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status run install uninstall help" -f -a "status" -d 'Show daemon status (running / not running, pid)'
complete -c monk -n "__fish_monk_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status run install uninstall help" -f -a "run" -d 'Run the daemon in the foreground (used by launchd / systemd; not normally invoked manually)'
complete -c monk -n "__fish_monk_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status run install uninstall help" -f -a "install" -d 'Install the daemon as a system service (asks for admin rights when needed)'
complete -c monk -n "__fish_monk_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status run install uninstall help" -f -a "uninstall" -d 'Remove the system service, asking for admin rights when needed (add --purge to also wipe data)'
complete -c monk -n "__fish_monk_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status run install uninstall help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from start" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from start" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from start" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from stop" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from stop" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from stop" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from status" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from status" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from status" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from run" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from run" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from run" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from install" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from install" -l reinstall -d 'Force reinstall (uninstall then install). Useful after a macOS major upgrade.'
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from install" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from install" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from uninstall" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from uninstall" -l purge -d 'Also delete config, audit log, and cached data'
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from uninstall" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from uninstall" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from help" -f -a "start" -d 'Start the background daemon (user mode)'
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from help" -f -a "stop" -d 'Stop the running daemon'
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from help" -f -a "status" -d 'Show daemon status (running / not running, pid)'
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from help" -f -a "run" -d 'Run the daemon in the foreground (used by launchd / systemd; not normally invoked manually)'
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from help" -f -a "install" -d 'Install the daemon as a system service (asks for admin rights when needed)'
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from help" -f -a "uninstall" -d 'Remove the system service, asking for admin rights when needed (add --purge to also wipe data)'
complete -c monk -n "__fish_monk_using_subcommand daemon; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c monk -n "__fish_monk_using_subcommand menubar; and not __fish_seen_subcommand_from install uninstall help" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand menubar; and not __fish_seen_subcommand_from install uninstall help" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand menubar; and not __fish_seen_subcommand_from install uninstall help" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand menubar; and not __fish_seen_subcommand_from install uninstall help" -f -a "install" -d 'Install the menu bar app as a login item'
complete -c monk -n "__fish_monk_using_subcommand menubar; and not __fish_seen_subcommand_from install uninstall help" -f -a "uninstall" -d 'Remove the menu bar app login item'
complete -c monk -n "__fish_monk_using_subcommand menubar; and not __fish_seen_subcommand_from install uninstall help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c monk -n "__fish_monk_using_subcommand menubar; and __fish_seen_subcommand_from install" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand menubar; and __fish_seen_subcommand_from install" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand menubar; and __fish_seen_subcommand_from install" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand menubar; and __fish_seen_subcommand_from uninstall" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand menubar; and __fish_seen_subcommand_from uninstall" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand menubar; and __fish_seen_subcommand_from uninstall" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand menubar; and __fish_seen_subcommand_from help" -f -a "install" -d 'Install the menu bar app as a login item'
complete -c monk -n "__fish_monk_using_subcommand menubar; and __fish_seen_subcommand_from help" -f -a "uninstall" -d 'Remove the menu bar app login item'
complete -c monk -n "__fish_monk_using_subcommand menubar; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c monk -n "__fish_monk_using_subcommand completions" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand completions" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand completions" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand update" -l locale -r -f -a "en\t''
ru\t''"
complete -c monk -n "__fish_monk_using_subcommand update" -l check -d 'Only check and report; do not download or install'
complete -c monk -n "__fish_monk_using_subcommand update" -s h -l help -d 'Print help'
complete -c monk -n "__fish_monk_using_subcommand update" -s V -l version -d 'Print version'
complete -c monk -n "__fish_monk_using_subcommand help; and not __fish_seen_subcommand_from init tui lang start stop panic status profiles profile apps stats doctor config daemon menubar completions update help" -f -a "init" -d 'Run the interactive onboarding wizard'
complete -c monk -n "__fish_monk_using_subcommand help; and not __fish_seen_subcommand_from init tui lang start stop panic status profiles profile apps stats doctor config daemon menubar completions update help" -f -a "tui" -d 'Open the interactive TUI'
complete -c monk -n "__fish_monk_using_subcommand help; and not __fish_seen_subcommand_from init tui lang start stop panic status profiles profile apps stats doctor config daemon menubar completions update help" -f -a "lang" -d 'Set the interface language'
complete -c monk -n "__fish_monk_using_subcommand help; and not __fish_seen_subcommand_from init tui lang start stop panic status profiles profile apps stats doctor config daemon menubar completions update help" -f -a "start" -d 'Start a focus session'
complete -c monk -n "__fish_monk_using_subcommand help; and not __fish_seen_subcommand_from init tui lang start stop panic status profiles profile apps stats doctor config daemon menubar completions update help" -f -a "stop" -d 'Stop the active session'
complete -c monk -n "__fish_monk_using_subcommand help; and not __fish_seen_subcommand_from init tui lang start stop panic status profiles profile apps stats doctor config daemon menubar completions update help" -f -a "panic" -d 'Request an escape from hard mode'
complete -c monk -n "__fish_monk_using_subcommand help; and not __fish_seen_subcommand_from init tui lang start stop panic status profiles profile apps stats doctor config daemon menubar completions update help" -f -a "status" -d 'Show daemon and session status'
complete -c monk -n "__fish_monk_using_subcommand help; and not __fish_seen_subcommand_from init tui lang start stop panic status profiles profile apps stats doctor config daemon menubar completions update help" -f -a "profiles" -d 'List profiles'
complete -c monk -n "__fish_monk_using_subcommand help; and not __fish_seen_subcommand_from init tui lang start stop panic status profiles profile apps stats doctor config daemon menubar completions update help" -f -a "profile" -d 'Manage profiles'
complete -c monk -n "__fish_monk_using_subcommand help; and not __fish_seen_subcommand_from init tui lang start stop panic status profiles profile apps stats doctor config daemon menubar completions update help" -f -a "apps" -d 'Manage installed applications cache'
complete -c monk -n "__fish_monk_using_subcommand help; and not __fish_seen_subcommand_from init tui lang start stop panic status profiles profile apps stats doctor config daemon menubar completions update help" -f -a "stats" -d 'Show session statistics'
complete -c monk -n "__fish_monk_using_subcommand help; and not __fish_seen_subcommand_from init tui lang start stop panic status profiles profile apps stats doctor config daemon menubar completions update help" -f -a "doctor" -d 'Check environment, permissions, and daemon health'
complete -c monk -n "__fish_monk_using_subcommand help; and not __fish_seen_subcommand_from init tui lang start stop panic status profiles profile apps stats doctor config daemon menubar completions update help" -f -a "config" -d 'Manage configuration'
complete -c monk -n "__fish_monk_using_subcommand help; and not __fish_seen_subcommand_from init tui lang start stop panic status profiles profile apps stats doctor config daemon menubar completions update help" -f -a "daemon" -d 'Manage the background daemon'
complete -c monk -n "__fish_monk_using_subcommand help; and not __fish_seen_subcommand_from init tui lang start stop panic status profiles profile apps stats doctor config daemon menubar completions update help" -f -a "menubar" -d 'Run the menu bar companion app (macOS only)'
complete -c monk -n "__fish_monk_using_subcommand help; and not __fish_seen_subcommand_from init tui lang start stop panic status profiles profile apps stats doctor config daemon menubar completions update help" -f -a "completions" -d 'Generate shell completions'
complete -c monk -n "__fish_monk_using_subcommand help; and not __fish_seen_subcommand_from init tui lang start stop panic status profiles profile apps stats doctor config daemon menubar completions update help" -f -a "update" -d 'Check GitHub releases for a new version and self-update'
complete -c monk -n "__fish_monk_using_subcommand help; and not __fish_seen_subcommand_from init tui lang start stop panic status profiles profile apps stats doctor config daemon menubar completions update help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c monk -n "__fish_monk_using_subcommand help; and __fish_seen_subcommand_from profile" -f -a "show" -d 'Show a profile\'s full configuration'
complete -c monk -n "__fish_monk_using_subcommand help; and __fish_seen_subcommand_from profile" -f -a "edit" -d 'Edit a profile (--add/--remove app id, or no flags for TTY editor)'
complete -c monk -n "__fish_monk_using_subcommand help; and __fish_seen_subcommand_from profile" -f -a "create" -d 'Create an empty profile or copy from a preset (`--preset deepwork`)'
complete -c monk -n "__fish_monk_using_subcommand help; and __fish_seen_subcommand_from profile" -f -a "duplicate" -d 'Duplicate an existing profile under a new name'
complete -c monk -n "__fish_monk_using_subcommand help; and __fish_seen_subcommand_from profile" -f -a "delete" -d 'Delete a profile'
complete -c monk -n "__fish_monk_using_subcommand help; and __fish_seen_subcommand_from profile" -f -a "limits" -d 'Set time limits for a profile (omit value to clear)'
complete -c monk -n "__fish_monk_using_subcommand help; and __fish_seen_subcommand_from apps" -f -a "list" -d 'List installed applications from cache'
complete -c monk -n "__fish_monk_using_subcommand help; and __fish_seen_subcommand_from apps" -f -a "scan" -d 'Force a rescan of installed applications'
complete -c monk -n "__fish_monk_using_subcommand help; and __fish_seen_subcommand_from config" -f -a "path"
complete -c monk -n "__fish_monk_using_subcommand help; and __fish_seen_subcommand_from config" -f -a "export"
complete -c monk -n "__fish_monk_using_subcommand help; and __fish_seen_subcommand_from config" -f -a "import"
complete -c monk -n "__fish_monk_using_subcommand help; and __fish_seen_subcommand_from daemon" -f -a "start" -d 'Start the background daemon (user mode)'
complete -c monk -n "__fish_monk_using_subcommand help; and __fish_seen_subcommand_from daemon" -f -a "stop" -d 'Stop the running daemon'
complete -c monk -n "__fish_monk_using_subcommand help; and __fish_seen_subcommand_from daemon" -f -a "status" -d 'Show daemon status (running / not running, pid)'
complete -c monk -n "__fish_monk_using_subcommand help; and __fish_seen_subcommand_from daemon" -f -a "run" -d 'Run the daemon in the foreground (used by launchd / systemd; not normally invoked manually)'
complete -c monk -n "__fish_monk_using_subcommand help; and __fish_seen_subcommand_from daemon" -f -a "install" -d 'Install the daemon as a system service (asks for admin rights when needed)'
complete -c monk -n "__fish_monk_using_subcommand help; and __fish_seen_subcommand_from daemon" -f -a "uninstall" -d 'Remove the system service, asking for admin rights when needed (add --purge to also wipe data)'
complete -c monk -n "__fish_monk_using_subcommand help; and __fish_seen_subcommand_from menubar" -f -a "install" -d 'Install the menu bar app as a login item'
complete -c monk -n "__fish_monk_using_subcommand help; and __fish_seen_subcommand_from menubar" -f -a "uninstall" -d 'Remove the menu bar app login item'

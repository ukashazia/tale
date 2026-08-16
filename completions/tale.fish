# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_tale_global_optspecs
    string join \n profile= config= view= read-only no-local tailscale-path= tailscale-socket= h/help V/version
end

function __fish_tale_needs_command
    # Figure out if the current invocation already has a command.
    set -l cmd (commandline -opc)
    set -e cmd[1]
    argparse -s (__fish_tale_global_optspecs) -- $cmd 2>/dev/null
    or return
    if set -q argv[1]
        # Also print the command, so this can be used to figure out what it is.
        echo $argv[1]
        return 1
    end
    return 0
end

function __fish_tale_using_subcommand
    set -l cmd (__fish_tale_needs_command)
    test -z "$cmd"
    and return 1
    contains -- $cmd[1] $argv
end

complete -c tale -n "__fish_tale_needs_command" -l profile -d 'Select a configured tailnet profile for this session' -r
complete -c tale -n "__fish_tale_needs_command" -l config -d 'Read configuration from PATH instead of the default config location' -r -F
complete -c tale -n "__fish_tale_needs_command" -l view -d 'Open ROUTE when the terminal interface starts' -r
complete -c tale -n "__fish_tale_needs_command" -l tailscale-path -d 'Use PATH as the local Tailscale executable' -r -F
complete -c tale -n "__fish_tale_needs_command" -l tailscale-socket -d 'Connect to the local Tailscale daemon at PATH' -r -F
complete -c tale -n "__fish_tale_needs_command" -l read-only -d 'Disable every mutation for this session'
complete -c tale -n "__fish_tale_needs_command" -l no-local -d 'Do not connect to the local Tailscale client or daemon'
complete -c tale -n "__fish_tale_needs_command" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_needs_command" -s V -l version -d 'Print version'
complete -c tale -n "__fish_tale_needs_command" -f -a "gen-completions" -d 'Print shell completion instructions to standard output'
complete -c tale -n "__fish_tale_needs_command" -f -a "auth" -d 'Add, inspect, or remove tailnet credentials'
complete -c tale -n "__fish_tale_needs_command" -f -a "config" -d 'Inspect and validate Tale configuration'
complete -c tale -n "__fish_tale_needs_command" -f -a "doctor" -d 'Print a redacted, non-mutating diagnostic report'
complete -c tale -n "__fish_tale_using_subcommand gen-completions" -l shell -d 'Shell to generate completions for: bash, zsh, or fish' -r
complete -c tale -n "__fish_tale_using_subcommand gen-completions" -l profile -d 'Select a configured tailnet profile for this session' -r
complete -c tale -n "__fish_tale_using_subcommand gen-completions" -l config -d 'Read configuration from PATH instead of the default config location' -r -F
complete -c tale -n "__fish_tale_using_subcommand gen-completions" -l view -d 'Open ROUTE when the terminal interface starts' -r
complete -c tale -n "__fish_tale_using_subcommand gen-completions" -l tailscale-path -d 'Use PATH as the local Tailscale executable' -r -F
complete -c tale -n "__fish_tale_using_subcommand gen-completions" -l tailscale-socket -d 'Connect to the local Tailscale daemon at PATH' -r -F
complete -c tale -n "__fish_tale_using_subcommand gen-completions" -l read-only -d 'Disable every mutation for this session'
complete -c tale -n "__fish_tale_using_subcommand gen-completions" -l no-local -d 'Do not connect to the local Tailscale client or daemon'
complete -c tale -n "__fish_tale_using_subcommand gen-completions" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -l profile -d 'Select a configured tailnet profile for this session' -r
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -l config -d 'Read configuration from PATH instead of the default config location' -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -l view -d 'Open ROUTE when the terminal interface starts' -r
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -l tailscale-path -d 'Use PATH as the local Tailscale executable' -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -l tailscale-socket -d 'Connect to the local Tailscale daemon at PATH' -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -l read-only -d 'Disable every mutation for this session'
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -l no-local -d 'Do not connect to the local Tailscale client or daemon'
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -f -a "add" -d 'Create or update a credential profile'
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -f -a "remove" -d 'Delete a credential profile and its stored secret'
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -f -a "status" -d 'Show the selected profile\'s credential status'
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l tailnet -d 'Tailnet ID or \'-\'; prompts when omitted' -r
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l kind -d 'Credential type: oauth-client or access-token; prompts when omitted' -r -f -a "oauth-client\t''
access-token\t''"
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l client-id -d 'OAuth client ID; required with --secret-stdin for oauth-client credentials' -r
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l scopes -d 'Comma-separated OAuth scopes; prompts when omitted for oauth-client credentials' -r
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l config -d 'Read configuration from PATH instead of the default config location' -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l view -d 'Open ROUTE when the terminal interface starts' -r
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l tailscale-path -d 'Use PATH as the local Tailscale executable' -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l tailscale-socket -d 'Connect to the local Tailscale daemon at PATH' -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l secret-stdin -d 'Read the secret from standard input instead of prompting. Selects the access token, or the client secret when the kind is `oauth_client`'
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l read-only -d 'Disable every mutation for this session'
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l no-local -d 'Do not connect to the local Tailscale client or daemon'
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from remove" -l config -d 'Read configuration from PATH instead of the default config location' -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from remove" -l view -d 'Open ROUTE when the terminal interface starts' -r
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from remove" -l tailscale-path -d 'Use PATH as the local Tailscale executable' -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from remove" -l tailscale-socket -d 'Connect to the local Tailscale daemon at PATH' -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from remove" -l read-only -d 'Disable every mutation for this session'
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from remove" -l no-local -d 'Do not connect to the local Tailscale client or daemon'
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from remove" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from status" -l config -d 'Read configuration from PATH instead of the default config location' -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from status" -l view -d 'Open ROUTE when the terminal interface starts' -r
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from status" -l tailscale-path -d 'Use PATH as the local Tailscale executable' -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from status" -l tailscale-socket -d 'Connect to the local Tailscale daemon at PATH' -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from status" -l read-only -d 'Disable every mutation for this session'
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from status" -l no-local -d 'Do not connect to the local Tailscale client or daemon'
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from status" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check show" -l profile -d 'Select a configured tailnet profile for this session' -r
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check show" -l config -d 'Read configuration from PATH instead of the default config location' -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check show" -l view -d 'Open ROUTE when the terminal interface starts' -r
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check show" -l tailscale-path -d 'Use PATH as the local Tailscale executable' -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check show" -l tailscale-socket -d 'Connect to the local Tailscale daemon at PATH' -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check show" -l read-only -d 'Disable every mutation for this session'
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check show" -l no-local -d 'Do not connect to the local Tailscale client or daemon'
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check show" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check show" -f -a "path" -d 'Print configuration, credential, state, and cache locations'
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check show" -f -a "check" -d 'Validate the configuration without opening the terminal interface'
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check show" -f -a "show" -d 'Every resolved value and what decided it'
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -l profile -d 'Select a configured tailnet profile for this session' -r
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -l config -d 'Read configuration from PATH instead of the default config location' -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -l view -d 'Open ROUTE when the terminal interface starts' -r
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -l tailscale-path -d 'Use PATH as the local Tailscale executable' -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -l tailscale-socket -d 'Connect to the local Tailscale daemon at PATH' -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -l read-only -d 'Disable every mutation for this session'
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -l no-local -d 'Do not connect to the local Tailscale client or daemon'
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -l profile -d 'Select a configured tailnet profile for this session' -r
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -l config -d 'Read configuration from PATH instead of the default config location' -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -l view -d 'Open ROUTE when the terminal interface starts' -r
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -l tailscale-path -d 'Use PATH as the local Tailscale executable' -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -l tailscale-socket -d 'Connect to the local Tailscale daemon at PATH' -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -l read-only -d 'Disable every mutation for this session'
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -l no-local -d 'Do not connect to the local Tailscale client or daemon'
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from show" -l profile -d 'Select a configured tailnet profile for this session' -r
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from show" -l config -d 'Read configuration from PATH instead of the default config location' -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from show" -l view -d 'Open ROUTE when the terminal interface starts' -r
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from show" -l tailscale-path -d 'Use PATH as the local Tailscale executable' -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from show" -l tailscale-socket -d 'Connect to the local Tailscale daemon at PATH' -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from show" -l read-only -d 'Disable every mutation for this session'
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from show" -l no-local -d 'Do not connect to the local Tailscale client or daemon'
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from show" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand doctor" -l output -d 'Write the report to PATH instead of standard output' -r -F
complete -c tale -n "__fish_tale_using_subcommand doctor" -l profile -d 'Select a configured tailnet profile for this session' -r
complete -c tale -n "__fish_tale_using_subcommand doctor" -l config -d 'Read configuration from PATH instead of the default config location' -r -F
complete -c tale -n "__fish_tale_using_subcommand doctor" -l view -d 'Open ROUTE when the terminal interface starts' -r
complete -c tale -n "__fish_tale_using_subcommand doctor" -l tailscale-path -d 'Use PATH as the local Tailscale executable' -r -F
complete -c tale -n "__fish_tale_using_subcommand doctor" -l tailscale-socket -d 'Connect to the local Tailscale daemon at PATH' -r -F
complete -c tale -n "__fish_tale_using_subcommand doctor" -l read-only -d 'Disable every mutation for this session'
complete -c tale -n "__fish_tale_using_subcommand doctor" -l no-local -d 'Do not connect to the local Tailscale client or daemon'
complete -c tale -n "__fish_tale_using_subcommand doctor" -s h -l help -d 'Print help'

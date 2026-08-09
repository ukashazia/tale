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

complete -c tale -n "__fish_tale_needs_command" -l profile -r
complete -c tale -n "__fish_tale_needs_command" -l config -r -F
complete -c tale -n "__fish_tale_needs_command" -l view -r
complete -c tale -n "__fish_tale_needs_command" -l tailscale-path -r -F
complete -c tale -n "__fish_tale_needs_command" -l tailscale-socket -r -F
complete -c tale -n "__fish_tale_needs_command" -l read-only
complete -c tale -n "__fish_tale_needs_command" -l no-local
complete -c tale -n "__fish_tale_needs_command" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_needs_command" -s V -l version -d 'Print version'
complete -c tale -n "__fish_tale_needs_command" -f -a "gen-completions" -d 'Print shell completion instructions to standard output'
complete -c tale -n "__fish_tale_needs_command" -f -a "auth"
complete -c tale -n "__fish_tale_needs_command" -f -a "config"
complete -c tale -n "__fish_tale_needs_command" -f -a "doctor"
complete -c tale -n "__fish_tale_using_subcommand gen-completions" -l shell -r
complete -c tale -n "__fish_tale_using_subcommand gen-completions" -l profile -r
complete -c tale -n "__fish_tale_using_subcommand gen-completions" -l config -r -F
complete -c tale -n "__fish_tale_using_subcommand gen-completions" -l view -r
complete -c tale -n "__fish_tale_using_subcommand gen-completions" -l tailscale-path -r -F
complete -c tale -n "__fish_tale_using_subcommand gen-completions" -l tailscale-socket -r -F
complete -c tale -n "__fish_tale_using_subcommand gen-completions" -l read-only
complete -c tale -n "__fish_tale_using_subcommand gen-completions" -l no-local
complete -c tale -n "__fish_tale_using_subcommand gen-completions" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -l profile -r
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -l config -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -l view -r
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -l tailscale-path -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -l tailscale-socket -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -l read-only
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -l no-local
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -f -a "add" -d '`auth add` is the only writer to the credential store, so it has to be usable without a terminal: the prompts cannot be reached from a script, a container, or a CI job, and they are the sole recovery path once a profile has been removed'
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -f -a "remove"
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -f -a "status"
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l tailnet -r
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l kind -r -f -a "oauth-client\t''
access-token\t''"
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l client-id -r
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l scopes -r
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l config -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l view -r
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l tailscale-path -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l tailscale-socket -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l secret-stdin -d 'Read the secret from standard input instead of prompting. Selects the access token, or the client secret when the kind is `oauth_client`'
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l read-only
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l no-local
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from remove" -l config -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from remove" -l view -r
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from remove" -l tailscale-path -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from remove" -l tailscale-socket -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from remove" -l read-only
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from remove" -l no-local
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from remove" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from status" -l config -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from status" -l view -r
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from status" -l tailscale-path -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from status" -l tailscale-socket -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from status" -l read-only
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from status" -l no-local
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from status" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check show" -l profile -r
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check show" -l config -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check show" -l view -r
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check show" -l tailscale-path -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check show" -l tailscale-socket -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check show" -l read-only
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check show" -l no-local
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check show" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check show" -f -a "path"
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check show" -f -a "check"
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check show" -f -a "show" -d 'Every resolved value and what decided it'
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -l profile -r
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -l config -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -l view -r
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -l tailscale-path -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -l tailscale-socket -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -l read-only
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -l no-local
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -l profile -r
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -l config -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -l view -r
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -l tailscale-path -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -l tailscale-socket -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -l read-only
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -l no-local
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from show" -l profile -r
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from show" -l config -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from show" -l view -r
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from show" -l tailscale-path -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from show" -l tailscale-socket -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from show" -l read-only
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from show" -l no-local
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from show" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand doctor" -l output -r -F
complete -c tale -n "__fish_tale_using_subcommand doctor" -l profile -r
complete -c tale -n "__fish_tale_using_subcommand doctor" -l config -r -F
complete -c tale -n "__fish_tale_using_subcommand doctor" -l view -r
complete -c tale -n "__fish_tale_using_subcommand doctor" -l tailscale-path -r -F
complete -c tale -n "__fish_tale_using_subcommand doctor" -l tailscale-socket -r -F
complete -c tale -n "__fish_tale_using_subcommand doctor" -l read-only
complete -c tale -n "__fish_tale_using_subcommand doctor" -l no-local
complete -c tale -n "__fish_tale_using_subcommand doctor" -s h -l help -d 'Print help'

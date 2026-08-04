# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_tale_global_optspecs
    string join \n profile= config= view= read-only no-local tailscale-path= h/help V/version
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
complete -c tale -n "__fish_tale_needs_command" -l read-only
complete -c tale -n "__fish_tale_needs_command" -l no-local
complete -c tale -n "__fish_tale_needs_command" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_needs_command" -s V -l version -d 'Print version'
complete -c tale -n "__fish_tale_needs_command" -f -a "auth"
complete -c tale -n "__fish_tale_needs_command" -f -a "config"
complete -c tale -n "__fish_tale_needs_command" -f -a "doctor"
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -l profile -r
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -l config -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -l view -r
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -l tailscale-path -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -l read-only
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -l no-local
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -f -a "add"
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -f -a "remove"
complete -c tale -n "__fish_tale_using_subcommand auth; and not __fish_seen_subcommand_from add remove status" -f -a "status"
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l config -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l view -r
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l tailscale-path -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l read-only
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -l no-local
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from add" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from remove" -l config -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from remove" -l view -r
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from remove" -l tailscale-path -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from remove" -l read-only
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from remove" -l no-local
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from remove" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from status" -l config -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from status" -l view -r
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from status" -l tailscale-path -r -F
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from status" -l read-only
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from status" -l no-local
complete -c tale -n "__fish_tale_using_subcommand auth; and __fish_seen_subcommand_from status" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check" -l profile -r
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check" -l config -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check" -l view -r
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check" -l tailscale-path -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check" -l read-only
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check" -l no-local
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check" -f -a "path"
complete -c tale -n "__fish_tale_using_subcommand config; and not __fish_seen_subcommand_from path check" -f -a "check"
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -l profile -r
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -l config -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -l view -r
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -l tailscale-path -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -l read-only
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -l no-local
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from path" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -l profile -r
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -l config -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -l view -r
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -l tailscale-path -r -F
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -l read-only
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -l no-local
complete -c tale -n "__fish_tale_using_subcommand config; and __fish_seen_subcommand_from check" -s h -l help -d 'Print help'
complete -c tale -n "__fish_tale_using_subcommand doctor" -l output -r -F
complete -c tale -n "__fish_tale_using_subcommand doctor" -l profile -r
complete -c tale -n "__fish_tale_using_subcommand doctor" -l config -r -F
complete -c tale -n "__fish_tale_using_subcommand doctor" -l view -r
complete -c tale -n "__fish_tale_using_subcommand doctor" -l tailscale-path -r -F
complete -c tale -n "__fish_tale_using_subcommand doctor" -l read-only
complete -c tale -n "__fish_tale_using_subcommand doctor" -l no-local
complete -c tale -n "__fish_tale_using_subcommand doctor" -s h -l help -d 'Print help'

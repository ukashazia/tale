_tale() {
    local i cur prev opts cmd
    COMPREPLY=()
    if [[ "${BASH_VERSINFO[0]}" -ge 4 ]]; then
        cur="$2"
    else
        cur="${COMP_WORDS[COMP_CWORD]}"
    fi
    prev="$3"
    cmd=""
    opts=""

    for i in "${COMP_WORDS[@]:0:COMP_CWORD}"
    do
        case "${cmd},${i}" in
            ",$1")
                cmd="tale"
                ;;
            tale,auth)
                cmd="tale__subcmd__auth"
                ;;
            tale,config)
                cmd="tale__subcmd__config"
                ;;
            tale,doctor)
                cmd="tale__subcmd__doctor"
                ;;
            tale__subcmd__auth,add)
                cmd="tale__subcmd__auth__subcmd__add"
                ;;
            tale__subcmd__auth,remove)
                cmd="tale__subcmd__auth__subcmd__remove"
                ;;
            tale__subcmd__auth,status)
                cmd="tale__subcmd__auth__subcmd__status"
                ;;
            tale__subcmd__config,check)
                cmd="tale__subcmd__config__subcmd__check"
                ;;
            tale__subcmd__config,path)
                cmd="tale__subcmd__config__subcmd__path"
                ;;
            *)
                ;;
        esac
    done

    case "${cmd}" in
        tale)
            opts="-h -V --profile --config --view --read-only --no-local --tailscale-path --tailscale-socket --help --version auth config doctor"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 1 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --profile)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --config)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --view)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tailscale-path)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tailscale-socket)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        tale__subcmd__auth)
            opts="-h --profile --config --view --read-only --no-local --tailscale-path --tailscale-socket --help add remove status"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --profile)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --config)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --view)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tailscale-path)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tailscale-socket)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        tale__subcmd__auth__subcmd__add)
            opts="-h --tailnet --kind --secret-stdin --client-id --scopes --config --view --read-only --no-local --tailscale-path --tailscale-socket --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --tailnet)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --kind)
                    COMPREPLY=($(compgen -W "oauth-client access-token" -- "${cur}"))
                    return 0
                    ;;
                --client-id)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --scopes)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --config)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --view)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tailscale-path)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tailscale-socket)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        tale__subcmd__auth__subcmd__remove)
            opts="-h --config --view --read-only --no-local --tailscale-path --tailscale-socket --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --config)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --view)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tailscale-path)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tailscale-socket)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        tale__subcmd__auth__subcmd__status)
            opts="-h --config --view --read-only --no-local --tailscale-path --tailscale-socket --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --config)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --view)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tailscale-path)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tailscale-socket)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        tale__subcmd__config)
            opts="-h --profile --config --view --read-only --no-local --tailscale-path --tailscale-socket --help path check"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --profile)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --config)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --view)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tailscale-path)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tailscale-socket)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        tale__subcmd__config__subcmd__check)
            opts="-h --profile --config --view --read-only --no-local --tailscale-path --tailscale-socket --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --profile)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --config)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --view)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tailscale-path)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tailscale-socket)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        tale__subcmd__config__subcmd__path)
            opts="-h --profile --config --view --read-only --no-local --tailscale-path --tailscale-socket --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --profile)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --config)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --view)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tailscale-path)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tailscale-socket)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        tale__subcmd__doctor)
            opts="-h --output --profile --config --view --read-only --no-local --tailscale-path --tailscale-socket --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --output)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --profile)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --config)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --view)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tailscale-path)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tailscale-socket)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
    esac
}

if [[ "${BASH_VERSINFO[0]}" -eq 4 && "${BASH_VERSINFO[1]}" -ge 4 || "${BASH_VERSINFO[0]}" -gt 4 ]]; then
    complete -F _tale -o nosort -o bashdefault -o default tale
else
    complete -F _tale -o bashdefault -o default tale
fi

Output docs/assets/.tale-devices.mp4

Set Shell "bash"
Set FontSize 17
Set FontFamily "JetBrainsMono Nerd Font Mono"
Set Width 1600
Set Height 880
Set LetterSpacing 0
Set LineHeight 1.15
Set Theme "Builtin Dark"
Set Padding 18
Set MarginFill "#E8E2DF"
Set Margin 32
Set BorderRadius 14
Set WindowBar "Colorful"
Set WindowBarSize 44
Set Framerate 15
Set TypingSpeed 1ms

Hide
Type "printf '\033]4;0;#202020\007'; env -u NO_COLOR COLORTERM=truecolor TERM=xterm-256color ./target/debug/tale --config /dev/null --view devices --mock"
Enter
Sleep 1400ms
Type "/"
Sleep 50ms
Type "dana"
Sleep 50ms
Enter
Sleep 100ms
Type "j"
Sleep 50ms
Type "i"
Sleep 100ms
Show
Sleep 200ms

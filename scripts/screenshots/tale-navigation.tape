Output docs/assets/.tale-navigation.mp4

Set Shell "bash"
Set FontSize 17
Set FontFamily "JetBrainsMono Nerd Font Mono"
Set Width 1600
Set Height 940
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

Hide
Type "env -u NO_COLOR COLORTERM=truecolor TERM=xterm-256color ./target/debug/tale --config /dev/null --view devices --mock"
Enter
Sleep 2s
Type ":"
Sleep 500ms
Show
Sleep 1200ms

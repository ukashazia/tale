{
  description = "Tale release package";

  inputs.nixpkgs.url = "https://api.flakehub.com/f/pinned/DeterminateSystems/nixpkgs-weekly/0.1.1042126%2Brev-624af665418d3c65d544145b4d34ad696439570e/019fcb6c-e772-7cb3-baa0-211e12b79e38/source.tar.gz";

  outputs = { nixpkgs, ... }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-darwin"
        "x86_64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "tale";
            version = "0.1.0";
            src = ../..;
            cargoLock.lockFile = ../../Cargo.lock;
            postInstall = ''
              install -Dm644 docs/cli/tale.1 $out/share/man/man1/tale.1
              install -Dm644 completions/tale.bash $out/share/bash-completion/completions/tale
              install -Dm644 completions/_tale $out/share/zsh/site-functions/_tale
              install -Dm644 completions/tale.fish $out/share/fish/vendor_completions.d/tale.fish
            '';

            meta = {
              description = "A keyboard-first terminal application for Tailscale networks";
              mainProgram = "tale";
              license = pkgs.lib.licenses.mit;
              platforms = systems;
            };
          };
        });
    };
}

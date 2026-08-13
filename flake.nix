{
  description = "Development environment for tale";

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
        devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = pkgs.mkShellNoCC {
            packages = with pkgs; [
              cargo-dist
              coreutils
              ffmpeg-headless
              vhs
            ];
          };
        });
    };
}

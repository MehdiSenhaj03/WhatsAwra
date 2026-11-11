self:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.whatsawra;
  json = pkgs.formats.json { };
  # paths.rs: ProjectDirs::from("me", "paolino", "whatsawra").
  configDir =
    if pkgs.stdenv.hostPlatform.isDarwin then
      "${config.home.homeDirectory}/Library/Application Support/me.paolino.whatsawra"
    else
      "${config.xdg.configHome}/whatsawra";
  declared = json.generate "whatsawra-settings.json" cfg.settings;
  jq = lib.getExe pkgs.jq;
  readSecret = key: path: ''
    if [[ -r ${lib.escapeShellArg path} ]]; then
      whatsawraSecrets+=(--rawfile ${lib.escapeShellArg key} ${lib.escapeShellArg path})
    else
      warnEcho ${lib.escapeShellArg "WhatsAwra: cannot read ${path}, keeping the current ${key}"}
    fi
  '';
in
{
  options.programs.whatsawra = {
    enable = lib.mkEnableOption "WhatsAwra, a native WhatsApp client";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "whatsawra.packages.\${pkgs.stdenv.hostPlatform.system}.default";
      description = "The WhatsAwra package to install.";
    };

    settings = lib.mkOption {
      type = lib.types.attrsOf json.type;
      default = { };
      example = lib.literalExpression ''
        {
          check_for_updates = false;
          message_sound = "none";
          theme = "dark";
        }
      '';
      description = ''
        Keys merged into WhatsAwra's `settings.json` on every activation. Keys
        left out keep whatever the app last saved, so window geometry, recent
        emoji and the lock verifiers survive. A key declared here is reset on
        the next activation if it is changed in the app. Per-account preferences
        such as read receipts live in each account's own file and cannot be
        declared here. Restart WhatsAwra after
        activation: it reads the file at start, and its next save writes back
        the values it holds in memory.

        Values are copied to the Nix store, which every local user can read.
        Give secrets, such as proxy credentials or `giphy_key`, through
        {option}`programs.whatsawra.secretFiles` instead.
      '';
    };

    secretFiles = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      example = lib.literalExpression ''
        {
          proxy = config.sops.secrets.whatsawra-proxy.path;
        }
      '';
      description = ''
        Keys of `settings.json` whose string values are read at activation from
        these files, for example secrets decrypted by sops-nix or agenix, so
        they never enter the Nix store. Give each path as a string: a Nix path
        literal would be copied to the store. A trailing newline is dropped. A
        file that cannot be read leaves that key as it is, with a warning; a
        secret manager that decrypts after activation fills it on the next
        activation.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = builtins.all (key: !(cfg.settings ? ${key})) (builtins.attrNames cfg.secretFiles);
        message = "programs.whatsawra: a key cannot be in both settings and secretFiles.";
      }
    ];

    home.packages = [ cfg.package ];

    # settings.json cannot be a store symlink: the app replaces it whole on
    # every change and keeps its own state in it.
    home.activation.whatsawraSettings = lib.mkIf (cfg.settings != { } || cfg.secretFiles != { }) (
      lib.hm.dag.entryAfter [ "writeBoundary" ] ''
        whatsawraDir=${lib.escapeShellArg configDir}
        whatsawraFile="$whatsawraDir/settings.json"
        run mkdir -p -m 700 "$whatsawraDir"
        if [[ ! -e "$whatsawraFile" ]]; then
          # A file without a version is migrated as one from older releases,
          # which would replace an explicit beige or black wallpaper.
          whatsawraBase='{"version":1}'
        elif whatsawraBase=$(${jq} -ce objects "$whatsawraFile" 2>/dev/null); then
          :
        else
          warnEcho "WhatsAwra: $whatsawraFile is not a JSON object, leaving it unchanged"
          whatsawraBase=
        fi
        if [[ -z "$whatsawraBase" ]]; then
          :
        elif [[ -v DRY_RUN ]]; then
          echo "Would merge ${declared} into $whatsawraFile"
        else
          whatsawraSecrets=()
          ${lib.concatStrings (lib.mapAttrsToList readSecret cfg.secretFiles)}
          whatsawraTmp=$(mktemp "$whatsawraDir/.settings.json.XXXXXX")
          ${jq} --slurpfile __whatsawraDeclared ${declared} "''${whatsawraSecrets[@]}" \
            '. * $__whatsawraDeclared[0] * ($ARGS.named | del(.__whatsawraDeclared) | map_values(rtrimstr("\n")))' \
            <<< "$whatsawraBase" > "$whatsawraTmp"
          chmod 600 "$whatsawraTmp"
          mv "$whatsawraTmp" "$whatsawraFile"
        fi
        unset whatsawraDir whatsawraFile whatsawraBase whatsawraSecrets whatsawraTmp
      ''
    );
  };
}

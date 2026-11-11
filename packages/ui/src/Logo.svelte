<script lang="ts">
  // The app icon from design/logo, used as it is (DESIGN.md 7.1).
  import icon from "../../../design/logo/figlock-icon.svg";

  let { size = 56 }: { size?: number } = $props();
</script>

<img src={icon} width={size} height={size} alt="FigLock" />

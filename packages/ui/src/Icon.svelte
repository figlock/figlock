<script lang="ts">
  import { ICONS, type IconName } from "./icons";

  type Props = { name: IconName; size?: number; class?: string };
  let { name, size = 16, class: klass = "" }: Props = $props();
</script>

<!-- The markup comes from the fixed table in icons.ts, never from data. -->
<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.5"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  focusable="false"
  class="flex-none {klass}"
>
  {@html ICONS[name]}
</svg>

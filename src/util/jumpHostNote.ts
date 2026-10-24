// What a delete question adds when servers that stay go through one of the
// servers being deleted as their jump host (#51): they won't connect until
// their user picks another jump host or chooses to connect directly
// (src-tauri/src/jump_gone.rs). Empty when there are none. Starts with a
// space, to follow the question's own words.
export const jumpHostNote = (servers: any[] | null | undefined, gone: Set<number>): string => {
  const all = servers || [];
  const behind = all.filter((x) => !gone.has(x.id) && x.jump_host_id != null && gone.has(x.jump_host_id));
  if (behind.length === 0) return "";
  const names = [...new Set(behind.map((x) => all.find((j) => j.id === x.jump_host_id)?.name || "it"))];
  const which = names.length === 1 ? names[0] : names.length === 2 ? `${names[0]} or ${names[1]}` : "one of these";
  const one = behind.length === 1;
  return ` ${behind.length} ${one ? "server uses" : "servers use"} ${which} as ${one ? "its" : "their"} jump host. ${
    one ? "It won't" : "They won't"
  } connect until you pick another jump host or choose to connect directly.`;
};

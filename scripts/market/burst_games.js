// Stake's « Burst Games » group → crates/slotlab-core/data/burst_games.json:
// the games Stake files as burst games, which the market reference leaves out
// (`slotlab_core::market::is_burst`, fetch_market.py, market_refresh.rs).
//
// stake.com turns a plain HTTP client away (Cloudflare, and an IP block in
// some countries), so the list is read in a browser where stake.com opens:
// paste this in the console of a stake.com tab. It downloads burst_games.json;
// put it in crates/slotlab-core/data/, rebuild SlotLab, and re-run
// scripts/market/fetch_market.py for the embedded market.
(async () => {
  const query = `query G($slug: String!, $limit: Int!, $offset: Int!) {
    slugKuratorGroup(slug: $slug) { gameCount groupGamesList(limit: $limit, offset: $offset) { game { slug } } }
  }`;
  const slugs = [];
  for (let offset = 0; ; offset += 50) {
    const r = await fetch('/_api/graphql', {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'x-language': 'en' },
      body: JSON.stringify({ query, variables: { slug: 'burst-games', limit: 50, offset } }),
    });
    const j = await r.json();
    if (j.errors) throw new Error(JSON.stringify(j.errors));
    const g = j.data.slugKuratorGroup;
    const page = g.groupGamesList.map((x) => x.game.slug);
    slugs.push(...page);
    if (page.length < 50 || slugs.length >= g.gameCount) break;
    await new Promise((res) => setTimeout(res, 250));
  }
  const sorted = [...new Set(slugs)].sort();
  // One slug a line, as the file in the repository (a refresh diffs cleanly).
  const text =
    '{\n"source": ' +
    JSON.stringify("stake.com — the Burst Games group (casino/group/burst-games), read through Stake's GraphQL in a browser") +
    ',\n"fetched": ' +
    JSON.stringify(new Date().toISOString().slice(0, 10)) +
    ',\n"slugs": [\n' +
    sorted.map((s) => JSON.stringify(s)).join(',\n') +
    '\n]\n}\n';
  const a = document.createElement('a');
  a.href = URL.createObjectURL(new Blob([text], { type: 'application/json' }));
  a.download = 'burst_games.json';
  a.click();
  console.log(`${sorted.length} burst games`);
})();

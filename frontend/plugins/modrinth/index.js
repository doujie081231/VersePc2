/**
 * VersePC - Minecraft Launcher
 * Copyright (c) 2026 豆杰
 * SPDX-License-Identifier: GPL-3.0-only
 */

const MODRINTH_API = 'https://api.modrinth.com/v2';

const PROJECT_TYPE_FACETS = {
    mod: 'project_type:mod',
    resourcepack: 'project_type:resourcepack',
    datapack: 'project_type:datapack',
    shader: 'project_type:shader'
};

const CATEGORY_LABELS = {
    fabric: 'Fabric', forge: 'Forge', neoforge: 'NeoForge', quilt: 'Quilt',
    liteloader: 'LiteLoader', modloader: 'Modloader', bukkit: 'Bukkit',
    spigot: 'Spigot', paper: 'Paper', purpur: 'Purpur'
};

async function execute(name, args, ctx) {
    const { httpGet } = ctx;

    if (name === 'search_modrinth') {
        const query = args.query || '';
        const limit = Math.min(Math.max(parseInt(args.limit) || 5, 1), 10);
        const facets = [];
        const typeKey = args.project_type || 'mod';
        if (PROJECT_TYPE_FACETS[typeKey]) facets.push(`[${JSON.stringify(PROJECT_TYPE_FACETS[typeKey])}]`);

        const params = new URLSearchParams({ query, limit: String(limit) });
        if (facets.length > 0) params.set('facets', `[${facets.join(',')}]`);

        let data;
        try {
            data = await httpGet(`${MODRINTH_API}/search?${params}`);
        } catch (e) {
            return JSON.stringify({ status: 'error', error: e.message });
        }
        if (!data || !data.hits) return JSON.stringify({ status: 'error', error: 'No results or API error' });

        const results = data.hits.map(hit => ({
            slug: hit.slug,
            title: hit.title,
            description: hit.description,
            downloads: hit.downloads,
            categories: hit.categories || [],
            versions: (hit.versions || []).slice(0, 3),
            author: hit.author || '',
            project_type: hit.project_type || '',
            installs: hit.installers || []
        }));

        return JSON.stringify({ status: 'data', count: results.length, results });
    }

    if (name === 'get_modrinth_info') {
        const idOrSlug = args.project_id_or_slug || '';
        let data;
        try {
            data = await httpGet(`${MODRINTH_API}/project/${encodeURIComponent(idOrSlug)}`);
        } catch (e) {
            return JSON.stringify({ status: 'error', error: e.message });
        }
        if (!data || data.error) return JSON.stringify({ status: 'error', error: data?.error || 'Project not found' });

        const project = {
            slug: data.slug,
            title: data.title,
            description: data.description,
            body: (data.body || '').slice(0, 1500),
            downloads: data.downloads,
            followers: data.followers,
            categories: data.categories || [],
            loaders: data.loaders || [],
            game_versions: (data.versions || []).slice(0, 5),
            license: data.license?.name || '',
            source_url: data.source_url || '',
            website_url: data.website_url || data.project_url || '',
            author: data.team || ''
        };

        return JSON.stringify({ status: 'data', project });
    }

    if (name === 'get_modrinth_versions') {
        const idOrSlug = args.project_id_or_slug || '';
        const params = new URLSearchParams();
        if (args.mc_version) params.set('game_versions', `["${args.mc_version}"]`);
        if (args.loader) params.set('loaders', `["${args.loader}"]`);
        params.set('limit', '10');

        let data;
        try {
            data = await httpGet(`${MODRINTH_API}/project/${encodeURIComponent(idOrSlug)}/version?${params}`);
        } catch (e) {
            return JSON.stringify({ status: 'error', error: e.message });
        }
        if (!Array.isArray(data)) return JSON.stringify({ status: 'error', error: 'Failed to fetch versions' });

        const versions = data.map(v => ({
            version_number: v.version_number,
            version_type: v.version_type,
            name: v.name,
            game_versions: v.game_versions || [],
            loaders: v.loaders || [],
            date_published: v.date_published,
            downloads: v.downloads,
            files: (v.files || []).map(f => ({ filename: f.filename, size: f.size }))
        }));

        return JSON.stringify({ status: 'data', count: versions.length, versions });
    }

    return JSON.stringify({ status: 'error', error: `Unknown tool: ${name}` });
}

module.exports = { execute };

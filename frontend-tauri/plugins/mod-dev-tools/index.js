const { getDevToolsManager } = require('../../dev-tools-manager');

function createPlugin() {
    const dtm = getDevToolsManager();

    return {
        tools: {
            check_dev_environment: {
                execute: async (args) => {
                    try {
                        const env = dtm.checkEnvironment();
                        return JSON.stringify(env, null, 2);
                    } catch (e) {
                        return JSON.stringify({ error: e.message });
                    }
                }
            },
            install_dev_tools: {
                execute: async (args) => {
                    try {
                        const components = args.components || ['jdk', 'gradle'];
                        const results = {};
                        if (components.includes('jdk')) {
                            results.jdk = await dtm.installJDK();
                        }
                        if (components.includes('gradle')) {
                            results.gradle = await dtm.installGradle();
                        }
                        if (components.includes('template') && args.loader && args.mcVersion) {
                            results.template = await dtm.downloadTemplate(args.loader, args.mcVersion);
                        }
                        return JSON.stringify(results, null, 2);
                    } catch (e) {
                        return JSON.stringify({ error: e.message });
                    }
                }
            },
            init_mod_project: {
                execute: async (args) => {
                    try {
                        const env = dtm.checkEnvironment();
                        if (!env.templates[args.loader + '-' + args.mcVersion]) {
                            const dlResult = await dtm.downloadTemplate(args.loader, args.mcVersion);
                            if (!dlResult.success) {
                                return JSON.stringify({ error: 'Template download failed: ' + (dlResult.error || 'unknown') });
                            }
                        }
                        const result = dtm.initProject({
                            modName: args.modName,
                            modId: args.modId,
                            loader: args.loader,
                            mcVersion: args.mcVersion,
                            packageName: args.packageName,
                            outputPath: args.outputPath
                        });
                        return JSON.stringify(result, null, 2);
                    } catch (e) {
                        return JSON.stringify({ error: e.message });
                    }
                }
            },
            build_mod: {
                execute: async (args) => {
                    try {
                        const result = dtm.buildMod(args.projectPath);
                        return JSON.stringify(result, null, 2);
                    } catch (e) {
                        return JSON.stringify({ error: e.message });
                    }
                }
            },
            create_datapack: {
                execute: async (args) => {
                    try {
                        const result = dtm.createDatapack({
                            mcVersion: args.mcVersion,
                            namespace: args.namespace,
                            items: args.items,
                            outputPath: args.outputPath
                        });
                        return JSON.stringify(result, null, 2);
                    } catch (e) {
                        return JSON.stringify({ error: e.message });
                    }
                }
            },
            create_resourcepack: {
                execute: async (args) => {
                    try {
                        const result = dtm.createResourcepack({
                            mcVersion: args.mcVersion,
                            namespace: args.namespace,
                            items: args.items,
                            outputPath: args.outputPath
                        });
                        return JSON.stringify(result, null, 2);
                    } catch (e) {
                        return JSON.stringify({ error: e.message });
                    }
                }
            },
            mod_compile_and_install: {
                execute: async (args) => {
                    try {
                        const result = dtm.compileAndInstall(args.projectPath, args.targetVersionId);
                        return JSON.stringify(result, null, 2);
                    } catch (e) {
                        return JSON.stringify({ error: e.message });
                    }
                }
            }
        }
    };
}

module.exports = { createPlugin };

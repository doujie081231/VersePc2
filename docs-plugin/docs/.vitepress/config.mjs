import { defineConfig } from 'vitepress'

export default defineConfig({
  // base 需与 Gitee Pages 仓库名一致；部署时可按实际情况调整
  base: '/verseplugin-docs/',
  title: 'VersePC2 插件开发文档',
  description: 'VersePC2 启动器插件开发指南：包格式、市场索引、打包与发布',
  lang: 'zh-CN',
  lastUpdated: true,
  cleanUrls: true,
  head: [
    ['link', { rel: 'icon', type: 'image/svg+xml', href: '/verseplugin-docs/favicon.svg' }]
  ],
  themeConfig: {
    logo: '/favicon.svg',
    nav: [
      { text: '指南', link: '/guide/' },
      { text: '功能插件', link: '/guide/feature-plugins' },
      { text: '个性化', link: '/guide/personalize' },
      { text: '提交审核', link: '/guide/publish' },
      { text: '插件仓库', link: 'https://gitee.com/doujie081231/verseplugin' }
    ],
    sidebar: [
      {
        text: '入门',
        items: [
          { text: '介绍', link: '/guide/' },
          { text: '快速开始', link: '/guide/quickstart' }
        ]
      },
      {
        text: '插件规范',
        items: [
          { text: 'plugin.json 字段', link: '/guide/plugin-json' },
          { text: '分类约定', link: '/guide/category' },
          { text: '市场索引 index.json', link: '/guide/index-json' }
        ]
      },
      {
        text: '开发类型',
        items: [
          { text: '功能插件（新增页面/卡片）', link: '/guide/feature-plugins' },
          { text: '个性化', link: '/guide/personalize' },
          { text: '示例插件工程', link: '/guide/example' }
        ]
      },
      {
        text: '发布',
        items: [
          { text: '打包与上传', link: '/guide/packaging' },
          { text: '提交审核与开源', link: '/guide/publish' },
          { text: '常见问题', link: '/guide/faq' }
        ]
      }
    ],
    search: {
      provider: 'local'
    }
  }
})
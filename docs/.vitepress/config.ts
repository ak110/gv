import { defineConfig } from 'vitepress'

export default defineConfig({
  lang: 'ja',
  title: 'ぐらびゅ',
  description: 'Windows用画像ビューアー',
  base: '/gv/',

  themeConfig: {
    nav: [
      { text: 'ホーム', link: '/' },
      { text: 'はじめに', link: '/guide/getting-started' },
    ],

    sidebar: [
      {
        text: 'ユーザーガイド',
        items: [
          { text: 'はじめに', link: '/guide/getting-started' },
          { text: '画像の表示', link: '/guide/viewing' },
          { text: 'ナビゲーション', link: '/guide/navigation' },
          { text: 'ファイル操作', link: '/guide/file-operations' },
          { text: '画像編集', link: '/guide/editing' },
          { text: '対応フォーマット', link: '/guide/formats' },
          { text: 'カスタマイズ', link: '/guide/customization' },
        ],
      },
      {
        text: '開発者向け',
        items: [
          { text: 'コンセプト', link: '/development/concept' },
          { text: 'アーキテクチャ', link: '/development/architecture' },
          { text: '開発ガイド', link: '/development/development' },
        ],
      },
    ],

    socialLinks: [
      { icon: 'github', link: 'https://github.com/ak110/gv' },
    ],

    search: {
      provider: 'local',
    },

    docFooter: {
      prev: '前のページ',
      next: '次のページ',
    },
    darkModeSwitchLabel: '外観',
    returnToTopLabel: 'トップに戻る',
    outline: {
      label: '目次',
    },
  },
})

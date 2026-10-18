import { defineConfig } from 'vitepress'

// 見出しIDの生成。VitePress 1.6.4の既定のslugify（@mdit-vue/sharedを同梱したもので、直接importできない）と
// 同じ変換の後に合成形（NFC）へ正規化する。既定の変換はNFKDで分解した後にラテン文字の結合記号（U+0300〜U+036F）
// だけを除くため、仮名の濁点・半濁点（U+3099・U+309A）が分解形のまま残り、合成形で書いた見出しへのリンクと一致しない。
// VitePressを更新したら、既定のslugifyとの差がNFCの正規化だけであることを確かめる。
const rControl = /[\u0000-\u001f]/g
const rSpecial = /[\s~`!@#$%^&*()\-_+=[\]{}|\\;:"'“”‘’<>,.?/]+/g
const rCombining = /[̀-ͯ]/g
const slugify = (str: string): string =>
  str
    .normalize('NFKD')
    .replace(rCombining, '')
    .replace(rControl, '')
    .replace(rSpecial, '-')
    .replace(/-{2,}/g, '-')
    .replace(/^-+|-+$/g, '')
    .replace(/^(\d)/, '_$1')
    .toLowerCase()
    .normalize('NFC')

export default defineConfig({
  lang: 'ja',
  title: 'ぐらびゅ',
  description: 'Windows用画像ビューアー',
  base: '/gv/',

  markdown: {
    anchor: { slugify },
  },

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

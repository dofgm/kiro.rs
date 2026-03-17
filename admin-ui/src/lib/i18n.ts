export type SupportedLocale = 'en' | 'zh-CN' | 'zh-TW' | 'ja' | 'ko'

const DEFAULT_LOCALE: SupportedLocale = 'en'

const MESSAGES: Record<
  SupportedLocale,
  {
    billingHeaderSetting: string
    billingHeaderSettingDesc: string
    billingHeaderEnabled: string
    billingHeaderDisabled: string
    billingHeaderEnableSuccess: string
    billingHeaderDisableSuccess: string
    billingHeaderUpdateErrorPrefix: string
    specialSettingsColumn: string
  }
> = {
  en: {
    billingHeaderSetting: 'Billing Header Rectifier',
    billingHeaderSettingDesc:
      'Strip x-anthropic-billing-header blocks from system prompt before forwarding.',
    billingHeaderEnabled: 'Enabled',
    billingHeaderDisabled: 'Disabled',
    billingHeaderEnableSuccess: 'Billing header rectifier enabled',
    billingHeaderDisableSuccess: 'Billing header rectifier disabled',
    billingHeaderUpdateErrorPrefix: 'Update failed: ',
    specialSettingsColumn: 'Special Settings',
  },
  'zh-CN': {
    billingHeaderSetting: 'Billing Header 清洗',
    billingHeaderSettingDesc:
      '转发前移除 system 中的 x-anthropic-billing-header 文本块。',
    billingHeaderEnabled: '已开启',
    billingHeaderDisabled: '已关闭',
    billingHeaderEnableSuccess: '已开启 Billing Header 清洗',
    billingHeaderDisableSuccess: '已关闭 Billing Header 清洗',
    billingHeaderUpdateErrorPrefix: '更新失败: ',
    specialSettingsColumn: '特殊设置',
  },
  'zh-TW': {
    billingHeaderSetting: 'Billing Header 清理',
    billingHeaderSettingDesc:
      '轉發前移除 system 中的 x-anthropic-billing-header 文字區塊。',
    billingHeaderEnabled: '已啟用',
    billingHeaderDisabled: '已停用',
    billingHeaderEnableSuccess: '已啟用 Billing Header 清理',
    billingHeaderDisableSuccess: '已停用 Billing Header 清理',
    billingHeaderUpdateErrorPrefix: '更新失敗: ',
    specialSettingsColumn: '特殊設定',
  },
  ja: {
    billingHeaderSetting: 'Billing Header Rectifier',
    billingHeaderSettingDesc:
      '転送前に system から x-anthropic-billing-header ブロックを削除します。',
    billingHeaderEnabled: '有効',
    billingHeaderDisabled: '無効',
    billingHeaderEnableSuccess: 'Billing Header Rectifier を有効化しました',
    billingHeaderDisableSuccess: 'Billing Header Rectifier を無効化しました',
    billingHeaderUpdateErrorPrefix: '更新失敗: ',
    specialSettingsColumn: '特別設定',
  },
  ko: {
    billingHeaderSetting: 'Billing Header Rectifier',
    billingHeaderSettingDesc:
      '업스트림 전송 전에 system 의 x-anthropic-billing-header 블록을 제거합니다.',
    billingHeaderEnabled: '활성화',
    billingHeaderDisabled: '비활성화',
    billingHeaderEnableSuccess: 'Billing Header Rectifier 활성화됨',
    billingHeaderDisableSuccess: 'Billing Header Rectifier 비활성화됨',
    billingHeaderUpdateErrorPrefix: '업데이트 실패: ',
    specialSettingsColumn: '특수 설정',
  },
}

export function normalizeLocale(value: string | null | undefined): SupportedLocale {
  if (!value) {
    return DEFAULT_LOCALE
  }

  const normalized = value.toLowerCase()
  if (normalized.startsWith('zh-cn') || normalized === 'zh-hans') {
    return 'zh-CN'
  }
  if (normalized.startsWith('zh-tw') || normalized.startsWith('zh-hk') || normalized === 'zh-hant') {
    return 'zh-TW'
  }
  if (normalized.startsWith('ja')) {
    return 'ja'
  }
  if (normalized.startsWith('ko')) {
    return 'ko'
  }
  if (normalized.startsWith('zh')) {
    return 'zh-CN'
  }
  return 'en'
}

export function getMessages(locale: string | null | undefined) {
  return MESSAGES[normalizeLocale(locale)]
}

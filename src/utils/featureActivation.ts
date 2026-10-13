import { IPC } from '../constants/ipc'
import type { FeatureActivation } from '../types/exotic'
import { invokeIpc } from './ipc'

/** 保存所选功能的免费激活记录，不提交注册码或购买信息。 */
export function activateFeature(featureId: string): Promise<FeatureActivation> {
  return invokeIpc(IPC.ACTIVATE_FEATURE, { featureId })
}

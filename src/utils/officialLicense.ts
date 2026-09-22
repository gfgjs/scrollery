import { IPC } from '../constants/ipc'
import { invokeIpc } from './ipc'

/** 激活固定的官方版商品，客户端不传授权主体或 SKU。 */
export function activateOfficialLicense(token: string): Promise<void> {
  return invokeIpc(IPC.ACTIVATE_OFFICIAL_LICENSE, { token })
}

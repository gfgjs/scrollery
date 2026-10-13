/** 执行具名场景；数量门从同一调用定义读取字面量身份。 */
export function testCase(_name, callback) {
  return callback();
}

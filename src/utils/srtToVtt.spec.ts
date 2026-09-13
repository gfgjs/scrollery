// srtToVtt 单测:正常转换 / 坏 cue 跳过 / CRLF / 已是 vtt / 标签保留与转义
import { describe, it, expect } from 'vitest'
import { srtToVtt, looksLikeVtt } from './srtToVtt'

describe('srtToVtt', () => {
  it('正常转换:编号剥离、时间戳逗号转点、多条 cue', () => {
    const srt = [
      '1',
      '00:00:01,000 --> 00:00:04,000',
      'Hello world',
      '',
      '2',
      '00:00:05,500 --> 00:00:07,250',
      'Second line',
      'with wrap',
    ].join('\n')

    const vtt = srtToVtt(srt)
    expect(vtt.startsWith('WEBVTT\n\n')).toBe(true)
    expect(vtt).toContain('00:00:01.000 --> 00:00:04.000')
    expect(vtt).toContain('Hello world')
    expect(vtt).toContain('00:00:05.500 --> 00:00:07.250')
    expect(vtt).toContain('Second line\nwith wrap')
  })

  it('坏 cue 跳过不抛:缺时间行、时间戳非法、无文本', () => {
    const srt = [
      '1',
      'not a time line',
      'orphan text',
      '',
      '2',
      'aa:bb:cc,ddd --> 00:00:07,250',
      'bad timestamp',
      '',
      '3',
      '00:00:08,000 --> 00:00:09,000',
      '',
      '4',
      '00:00:10,000 --> 00:00:11,000',
      'valid cue',
    ].join('\n')

    expect(() => srtToVtt(srt)).not.toThrow()
    const vtt = srtToVtt(srt)
    expect(vtt).toContain('valid cue')
    expect(vtt).not.toContain('orphan text')
    expect(vtt).not.toContain('bad timestamp')
  })

  it('CRLF 与 LF 均可处理', () => {
    const srtCrlf = '1\r\n00:00:01,000 --> 00:00:02,000\r\nCRLF text\r\n'
    const vtt = srtToVtt(srtCrlf)
    expect(vtt).toContain('00:00:01.000 --> 00:00:02.000')
    expect(vtt).toContain('CRLF text')
  })

  it('小时/毫秒补零', () => {
    const srt = '1\n0:01:02,3 --> 0:01:05,30\ntext'
    const vtt = srtToVtt(srt)
    expect(vtt).toContain('00:01:02.300 --> 00:01:05.300')
  })

  it('标签保留:<b> <i> <u> 不转义', () => {
    const srt = '1\n00:00:01,000 --> 00:00:02,000\n<b>bold</b> <i>italic</i> <u>underline</u>'
    const vtt = srtToVtt(srt)
    expect(vtt).toContain('<b>bold</b> <i>italic</i> <u>underline</u>')
  })

  it('白名单标签含属性时剥除属性', () => {
    const srt = '1\n00:00:01,000 --> 00:00:02,000\n<b class="x">text</b>'
    const vtt = srtToVtt(srt)
    expect(vtt).toContain('<b>text</b>')
    expect(vtt).not.toContain('class')
  })

  it('白名单标签大小写归一', () => {
    const srt = '1\n00:00:01,000 --> 00:00:02,000\n<B>text</B>'
    const vtt = srtToVtt(srt)
    expect(vtt).toContain('<b>text</b>')
  })

  it('其余尖括号转义防注入', () => {
    const srt = '1\n00:00:01,000 --> 00:00:02,000\n<script>alert(1)</script>'
    const vtt = srtToVtt(srt)
    expect(vtt).not.toContain('<script>')
    expect(vtt).toContain('&lt;script&gt;alert(1)&lt;/script&gt;')
  })

  it('空输入返回仅含头部', () => {
    expect(srtToVtt('')).toBe('WEBVTT\n\n')
  })
})

describe('looksLikeVtt', () => {
  it('首个非空行以 WEBVTT 开头 → true', () => {
    expect(looksLikeVtt('WEBVTT\n\n00:00:01.000 --> 00:00:02.000\ntext')).toBe(true)
    expect(looksLikeVtt('\n\n  WEBVTT\nfoo')).toBe(true)
  })

  it('非 WEBVTT 开头 → false', () => {
    expect(looksLikeVtt('1\n00:00:01,000 --> 00:00:02,000\ntext')).toBe(false)
    expect(looksLikeVtt('')).toBe(false)
  })
})

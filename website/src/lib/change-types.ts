export type ChangeType = 'feature' | 'improvement' | 'fix' | 'performance' | 'docs' | 'packaging' | 'removed' | 'maintenance';

/** Explicit section categories take priority; topical sections use the entry's action. */
export function changeType(section: string, entry: string): ChangeType {
  if (/^(移除|删除|Remove\b)/i.test(entry)) return 'removed';
  if (/^(新增功能|新功能|New features)$/i.test(section)) return 'feature';
  if (/^(问题修复|修复|Fixes)$/i.test(section)) return 'fix';
  if (/^(性能优化|性能|Performance)$/i.test(section)) return 'performance';
  if (/^(使用说明|Documentation)$/i.test(section)) return 'docs';
  if (/^(安装与发布|发布流程|Packaging|Release process)$/i.test(section)) return 'packaging';
  if (/^(质量保障|维护更新|Quality checks|Maintenance)$/i.test(section)) return 'maintenance';
  if (/^(修复|解决|Fix\b|Resolve\b)/i.test(entry)) return 'fix';
  if (/^(新增|增加|Add\b|Introduce\b)/i.test(entry)) return 'feature';
  if (/(减少.*(?:开销|内存占用)|降低.*(?:开销|内存占用)|Reduce.*(?:overhead|memory usage))/i.test(entry)) return 'performance';
  return 'improvement';
}

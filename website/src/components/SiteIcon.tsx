import { Download, LayoutGrid, Search, Focus, ChevronRight, ChevronDown, CodeXml, ArrowUpRight } from 'lucide-react';
export const icons = { download: Download, 'layout-grid': LayoutGrid, search: Search, focus: Focus, 'chevron-right': ChevronRight, 'chevron-down': ChevronDown, github: CodeXml, 'arrow-up-right': ArrowUpRight };
export default function SiteIcon({name, className}: {name?: string; className?: string}) {
  const Icon = icons[name as keyof typeof icons];
  return Icon ? <Icon className={className} aria-hidden="true" /> : null;
}

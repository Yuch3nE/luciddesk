import { Download, LayoutGrid, Search, Focus, ChevronRight } from 'lucide-react';
export const icons = { download: Download, 'layout-grid': LayoutGrid, search: Search, focus: Focus, 'chevron-right': ChevronRight };
export default function SiteIcon({name, className}: {name?: string; className?: string}) {
  const Icon = icons[name as keyof typeof icons];
  return Icon ? <Icon className={className} aria-hidden="true" /> : null;
}

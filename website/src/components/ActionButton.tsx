import { Button } from './ui/button';
import SiteIcon from './SiteIcon';
import type { ComponentProps } from 'react';
type Props = { text: string; icon?: string; href?: string; target?: string; className?: string; variant?: 'primary' | 'secondary' | 'tertiary' | 'link'; type?: 'button' | 'submit' | 'reset' };
export default function ActionButton({text, icon, href, target, className, variant = 'secondary', type}: Props) {
  const style: ComponentProps<typeof Button>['variant'] = ({primary:'default',secondary:'outline',tertiary:'ghost',link:'link'} as const)[variant];
  const variantExtras = variant === 'primary'
    ? 'shadow-sm shadow-primary/20 hover:shadow-md hover:shadow-primary/25'
    : variant === 'secondary'
      ? 'border-line bg-elevated text-heading hover:bg-surface'
      : '';
  const content = <><span dangerouslySetInnerHTML={{__html:text}} />{icon && <SiteIcon name={icon} className="size-4.5" />}</>;
  const classes = `min-h-11 h-auto rounded-xl px-5 py-2.5 text-sm font-semibold ${variantExtras} ${className || ''}`;
  return type ? <Button variant={style} type={type} className={classes}>{content}</Button> : <Button asChild variant={style} className={classes}><a href={href} target={target} rel={target ? 'noopener noreferrer' : undefined}>{content}</a></Button>;
}

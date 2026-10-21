import type { HTMLAttributes } from 'astro/types';

export interface Image {
  src: string;
  alt?: string;
}

export interface Widget {
  id?: string;
  isDark?: boolean;
  bg?: string;
  classes?: Record<string, string | Record<string, string>>;
}

export interface Headline {
  title?: string;
  subtitle?: string;
  tagline?: string;
  classes?: Record<string, string>;
}

export interface Item {
  title?: string;
  description?: string;
  icon?: string;
  classes?: Record<string, string>;
  callToAction?: CallToAction;
}

export interface CallToAction extends Omit<HTMLAttributes<'a'>, 'slot'> {
  variant?: 'primary' | 'secondary' | 'tertiary' | 'link';
  text?: string;
  icon?: string;
  classes?: Record<string, string>;
  type?: 'button' | 'submit' | 'reset';
}

export interface HeroImage extends Image {
}

export interface Hero extends Omit<Headline, 'classes'>, Omit<Widget, 'isDark' | 'classes'> {
  badge?: string;
  content?: string;
  actions?: string | CallToAction[];
  image?: string | HeroImage;
}

export interface Features extends Omit<Headline, 'classes'>, Widget {
  items?: Array<Item>;
  columns?: number;
  defaultIcon?: string;
}

export interface Faqs extends Headline {
  id?: string;
  items?: Array<Pick<Item, 'title' | 'description'>>;
}

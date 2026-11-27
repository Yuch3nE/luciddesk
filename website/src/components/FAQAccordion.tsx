import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from './ui/accordion';
export default function FAQAccordion({items}: {items: {title?: string; description?: string}[]}) {
  return <Accordion type="single" collapsible className="w-full">
    {items.map((item,index) => <AccordionItem key={item.title} value={`faq-${index}`} className="border-line">
      <AccordionTrigger className="items-center py-5 text-base font-semibold text-heading transition-colors hover:text-primary hover:no-underline dark:hover:text-blue-300">{item.title}</AccordionTrigger>
      <AccordionContent className="pr-6 pb-5 text-sm leading-7 text-muted [&_a]:font-medium [&_a]:text-primary [&_a]:underline [&_a]:underline-offset-4 dark:[&_a]:text-blue-300"><div dangerouslySetInnerHTML={{__html:item.description || ''}}/></AccordionContent>
    </AccordionItem>)}
  </Accordion>;
}

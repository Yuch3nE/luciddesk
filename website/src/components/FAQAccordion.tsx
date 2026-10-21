import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from './ui/accordion';
export default function FAQAccordion({items}: {items: {title?: string; description?: string}[]}) {
  return <Accordion type="single" collapsible className="mx-auto max-w-3xl">
    {items.map((item,index) => <AccordionItem key={item.title} value={`faq-${index}`} className="border-line">
      <AccordionTrigger className="items-center py-5 text-base font-semibold text-heading hover:no-underline">{item.title}</AccordionTrigger>
      <AccordionContent className="pr-8 text-sm leading-7 text-muted [&_a]:underline [&_a]:underline-offset-4"><div dangerouslySetInnerHTML={{__html:item.description || ''}}/></AccordionContent>
    </AccordionItem>)}
  </Accordion>;
}

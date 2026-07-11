// Business letter
#import "shared/letter.typ": letter

#show: letter.with(
  name: "Jane Roe",
  address: [123 Main St \ Portland, OR 97201],
  contact: (email: "jane@roe.com", phone: "+1 555 0100"),
  recipient: [Acme Corp. \ Sesame Street 23 \ 12345 Gotham City],
  subject: [Re: Your inquiry],
  signature: "/shared/signature.png",
)

Dear Acme team,

Lorem ipsum dolor sit amet, consectetuer adipiscing elit.

Sincerely,

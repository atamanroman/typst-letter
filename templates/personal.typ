// Personal note
#import "shared/letter.typ": letter

#show: letter.with(
  name: "Jane",
  contact: (email: "jane@roe.com"),
  recipient: [Dear Alex,],
  date: datetime.today().display("[day].[month].[year]"),
  closing: "Warmly,",
)

It has been far too long since we last spoke. I hope this letter
finds you well.

Write back soon!

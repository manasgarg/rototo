function register(lint)
  lint:rule({
    id = "payments/check",
    title = "Payments check",
    help = "Fix the payments policy.",
    target = "list=tier",
    handler = "check",
  })
end

function check(package, target)
  return {}
end
